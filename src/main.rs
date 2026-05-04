use askama::Template;
use clap::{Parser, Subcommand};
use dotenvy::from_filename;
use env_logger;
use log::{error, info};
use parsers::port::PortList;
use std::collections::HashSet;
use std::env;
use std::process::exit;
#[cfg(feature = "nixos")]
mod config;
mod format;
#[cfg(feature = "nixos")]
mod install;
mod parsers;
use parsers::*;
#[allow(unused_imports)]
use std::net::IpAddr;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "RouterConfig")]
#[command(about = "Generates router configuration from environment or .env file")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive configuration menu
    #[cfg(feature = "nixos")]
    Config,

    /// Install nifty-filter to disk from the live ISO
    #[cfg(feature = "nixos")]
    Install {
        /// Set a git remote for config updates
        #[arg(long)]
        git_remote: Option<String>,
    },

    /// Reboot into maintenance mode (read-write root)
    #[cfg(feature = "nixos")]
    Maintenance,

    /// Upgrade the system in place
    #[cfg(feature = "nixos")]
    Upgrade {
        /// Target branch (overrides saved branch)
        branch: Option<String>,
    },

    /// Print version and build info
    Version,

    /// Generate nftables configuration
    #[command(alias = "nft")]
    Nftables {
        /// Path to the .env file (actual environment vars supersede this)
        #[arg(long)]
        env_file: Option<String>,

        /// Ignore the environment (combine this with --env-file)
        #[arg(long)]
        strict_env: bool,

        /// Validate with nft -c (only works if interfaces exist on this host)
        #[arg(long)]
        validate: bool,

        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Template)]
#[template(path = "router.nft.txt")]
struct RouterTemplate {
    interface_lan: Interface,
    interface_wan: Interface,
    interface_mgmt: String,
    subnet_mgmt_ipv4: String,

    // Protocol enablement
    enable_ipv4: bool,
    enable_ipv6: bool,

    // IPv4 subnet (required when enable_ipv4=true)
    subnet_lan_ipv4: String,
    // IPv6 subnet (required when enable_ipv6=true)
    subnet_lan_ipv6: String,

    // ICMPv4
    icmp_accept_lan: String,
    icmp_accept_wan: String,

    // ICMPv6
    icmpv6_accept_lan: String,
    icmpv6_accept_wan: String,

    // Port accepts (protocol-agnostic)
    tcp_accept_lan: String,
    udp_accept_lan: String,
    tcp_accept_wan: String,
    udp_accept_wan: String,

    // Forward routes
    tcp_forward_lan: ForwardRouteList,
    udp_forward_lan: ForwardRouteList,
    tcp_forward_wan: ForwardRouteList,
    udp_forward_wan: ForwardRouteList,

    // Egress filtering
    lan_egress_allowed_ipv4: String,
    lan_egress_allowed_ipv6: String,
}

impl RouterTemplate {
    fn from_env() -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let interface_lan = get_interface("INTERFACE_LAN", &mut errors);
        let interface_wan = get_interface("INTERFACE_WAN", &mut errors);
        let interface_mgmt = env::var("INTERFACE_MGMT").unwrap_or_default();
        let subnet_mgmt_ipv4 = if !interface_mgmt.is_empty() {
            match get_subnet_optional("SUBNET_MGMT", &mut errors) {
                Some(s) => s.to_string(),
                None => {
                    errors.push(
                        "SUBNET_MGMT is required when INTERFACE_MGMT is set.".to_string(),
                    );
                    String::new()
                }
            }
        } else {
            String::new()
        };

        let enable_ipv4 = get_bool("ENABLE_IPV4", &mut errors, Some(true));
        let enable_ipv6 = get_bool("ENABLE_IPV6", &mut errors, Some(false));

        if !enable_ipv4 && !enable_ipv6 {
            errors.push("At least one of ENABLE_IPV4 or ENABLE_IPV6 must be true.".to_string());
        }

        // IPv4 subnet: try SUBNET_LAN_IPV4, fall back to SUBNET_LAN for backward compat
        let subnet_lan_ipv4 = if enable_ipv4 {
            let subnet = get_subnet_optional("SUBNET_LAN_IPV4", &mut errors)
                .or_else(|| get_subnet_optional("SUBNET_LAN", &mut errors));
            match subnet {
                Some(s) => s.to_string(),
                None => {
                    errors.push(
                        "SUBNET_LAN_IPV4 (or SUBNET_LAN) is required when ENABLE_IPV4=true."
                            .to_string(),
                    );
                    String::new()
                }
            }
        } else {
            String::new()
        };

        // IPv6 subnet
        let subnet_lan_ipv6 = if enable_ipv6 {
            match get_subnet_optional("SUBNET_LAN_IPV6", &mut errors) {
                Some(s) => s.to_string(),
                None => {
                    errors.push("SUBNET_LAN_IPV6 is required when ENABLE_IPV6=true.".to_string());
                    String::new()
                }
            }
        } else {
            String::new()
        };

        // ICMPv4
        let icmp_accept_lan = if enable_ipv4 {
            IcmpType::vec_to_string(&get_icmp_types(
                "ICMP_ACCEPT_LAN",
                &mut errors,
                vec![
                    IcmpType::EchoRequest,
                    IcmpType::EchoReply,
                    IcmpType::DestinationUnreachable,
                    IcmpType::TimeExceeded,
                ],
            ))
        } else {
            String::new()
        };
        let icmp_accept_wan = if enable_ipv4 {
            IcmpType::vec_to_string(&get_icmp_types("ICMP_ACCEPT_WAN", &mut errors, vec![]))
        } else {
            String::new()
        };

        // ICMPv6
        let icmpv6_accept_lan = if enable_ipv6 {
            Icmpv6Type::vec_to_string(&get_icmpv6_types(
                "ICMPV6_ACCEPT_LAN",
                &mut errors,
                vec![
                    Icmpv6Type::NdNeighborSolicit,
                    Icmpv6Type::NdNeighborAdvert,
                    Icmpv6Type::NdRouterSolicit,
                    Icmpv6Type::NdRouterAdvert,
                    Icmpv6Type::EchoRequest,
                    Icmpv6Type::EchoReply,
                ],
            ))
        } else {
            String::new()
        };
        let icmpv6_accept_wan = if enable_ipv6 {
            Icmpv6Type::vec_to_string(&get_icmpv6_types(
                "ICMPV6_ACCEPT_WAN",
                &mut errors,
                vec![
                    Icmpv6Type::NdNeighborSolicit,
                    Icmpv6Type::NdNeighborAdvert,
                    Icmpv6Type::DestinationUnreachable,
                    Icmpv6Type::PacketTooBig,
                    Icmpv6Type::TimeExceeded,
                ],
            ))
        } else {
            String::new()
        };

        // Port accepts (protocol-agnostic)
        let tcp_accept_lan = get_port_accept(
            "TCP_ACCEPT_LAN",
            &mut errors,
            PortList::new("22,80,443").unwrap(),
        )
        .to_string();
        let udp_accept_lan =
            get_port_accept("UDP_ACCEPT_LAN", &mut errors, PortList::new("").unwrap()).to_string();
        let tcp_accept_wan =
            get_port_accept("TCP_ACCEPT_WAN", &mut errors, PortList::new("").unwrap()).to_string();
        let udp_accept_wan =
            get_port_accept("UDP_ACCEPT_WAN", &mut errors, PortList::new("").unwrap()).to_string();

        // Forward routes
        let tcp_forward_lan = get_forward_routes(
            "TCP_FORWARD_LAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let udp_forward_lan = get_forward_routes(
            "UDP_FORWARD_LAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let tcp_forward_wan = get_forward_routes(
            "TCP_FORWARD_WAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let udp_forward_wan = get_forward_routes(
            "UDP_FORWARD_WAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );

        // Egress filtering
        let lan_egress_allowed_ipv4 = if enable_ipv4 {
            get_cidr_list(
                "LAN_EGRESS_ALLOWED_IPV4",
                &mut errors,
                CidrList::new("0.0.0.0/0").unwrap(),
            )
            .to_string()
        } else {
            String::new()
        };
        let lan_egress_allowed_ipv6 = if enable_ipv6 {
            get_cidr_list(
                "LAN_EGRESS_ALLOWED_IPV6",
                &mut errors,
                CidrList::new("::/0").unwrap(),
            )
            .to_string()
        } else {
            String::new()
        };

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_lan,
            interface_wan,
            interface_mgmt,
            subnet_mgmt_ipv4,
            enable_ipv4,
            enable_ipv6,
            subnet_lan_ipv4,
            subnet_lan_ipv6,
            icmp_accept_lan,
            icmp_accept_wan,
            icmpv6_accept_lan,
            icmpv6_accept_wan,
            tcp_accept_lan,
            udp_accept_lan,
            tcp_accept_wan,
            udp_accept_wan,
            tcp_forward_lan,
            udp_forward_lan,
            tcp_forward_wan,
            udp_forward_wan,
            lan_egress_allowed_ipv4,
            lan_egress_allowed_ipv6,
        })
    }
}

pub fn validate_nftables_config(config: &str) -> Result<(), String> {
    let output = Command::new("nft")
        .arg("-c")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                stdin.write_all(config.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("Failed to run nft command: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[cfg(feature = "nixos")]
fn run_maintenance() {
    use std::process::{exit, Command};

    // Write the embedded script to a temp file so it can be executed directly.
    // This ensures `$0` in the script is the actual file path (not "sh"),
    // which is required for the `exec sudo "$0" "$@"` privilege escalation.
    let script = include_str!("../nix/nifty-maintenance.sh");
    let tmp = std::env::temp_dir().join("nifty-maintenance.sh");
    std::fs::write(&tmp, script).expect("failed to write temp file");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .expect("failed to chmod temp file");

    let status = Command::new(tmp)
        .status()
        .expect("failed to execute maintenance script");
    exit(status.code().unwrap_or(1));
}

#[cfg(feature = "nixos")]
fn run_upgrade(branch: Option<String>) {
    use std::process::{exit, Command};

    // Write the embedded script to a temp file so it can be executed directly
    // This ensures `$0` in the script is the actual file path (not "sh"),
    // which is required for the `exec sudo "$0" "$@"` privilege escalation
    let script = include_str!("../nix/nifty-upgrade.sh");
    let tmp = std::env::temp_dir().join("nifty-upgrade.sh");
    std::fs::write(&tmp, script).expect("failed to write temp file");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .expect("failed to chmod temp file");

    let mut cmd = Command::new(&tmp);
    cmd.env("TMPDIR", "/var/tmp");
    if let Some(b) = branch {
        cmd.arg(b);
    }
    let status = cmd.status().expect("failed to execute upgrade script");
    exit(status.code().unwrap_or(1));
}

fn app() {
    // Parse command-line arguments
    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "nixos")]
        Commands::Config => config::run(),
        #[cfg(feature = "nixos")]
        Commands::Install { git_remote } => install::run(git_remote),
        #[cfg(feature = "nixos")]
        Commands::Maintenance => run_maintenance(),
        #[cfg(feature = "nixos")]
        Commands::Upgrade { branch } => run_upgrade(branch),
        Commands::Version => {
            println!("nifty-filter {} ({})", env!("CARGO_PKG_VERSION"), option_env!("GIT_SHA").unwrap_or("unknown"));
        }
        Commands::Nftables {
            env_file,
            strict_env,
            validate,
            verbose,
        } => {
            // Set RUST_LOG to info if verbose is enabled
            if verbose {
                env::set_var("RUST_LOG", "info");
            }

            // Initialize the logger
            env_logger::init();

            // Ignore non-default environment variables if `--strict-env` is set
            if env_file.is_some() && strict_env {
                let default_vars: HashSet<&str> = [
                    "HOME", "USER", "PWD", "OLDPWD", "SHELL", "PATH", "LANG", "TERM", "UID",
                    "EUID", "LOGNAME", "HOSTNAME", "EDITOR", "VISUAL",
                ]
                .iter()
                .cloned()
                .collect();

                for (key, _) in env::vars() {
                    if !default_vars.contains(key.as_str())
                        && !key.starts_with("RUST")
                        && !key.starts_with("CARGO")
                    {
                        env::remove_var(&key);
                    }
                }
            }

            // Load the specified .env file if provided
            if let Some(env_file) = env_file {
                match from_filename(&env_file) {
                    Ok(_) => {
                        info!("Loaded environment from file: {}", env_file);
                    }
                    Err(err) => {
                        error!("Error parsing {} : {}", env_file, err);
                        error!("Failed to load environment from file: {}", env_file);
                        exit(1);
                    }
                }
            }

            // Attempt to create the RouterTemplate from environment variables
            match RouterTemplate::from_env() {
                Ok(router) => {
                    let text = format::reduce_blank_lines(&router.render().unwrap());
                    if validate {
                        match validate_nftables_config(&text) {
                            Ok(_valid) => {}
                            Err(e) => {
                                error!("Error validating nftables config: {}", e);
                                exit(1)
                            }
                        }
                    }
                    println!("{}", text)
                }
                Err(errors) => {
                    for err in errors {
                        eprintln!("Error: {}", err);
                    }
                    std::process::exit(1);
                }
            }
        }
    }
}

fn main() {
    app()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward_route_parsing() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(route_list.get_routes().len(), 2);
        assert_eq!(route_list.get_routes()[0].incoming_port, 8080);
        assert_eq!(
            route_list.get_routes()[0].destination_ip,
            "192.168.1.100".parse::<IpAddr>().unwrap()
        );
        assert_eq!(route_list.get_routes()[0].destination_port, 80);
    }

    #[test]
    fn test_forward_route_invalid() {
        let input = "8080:192.168.1.100, 8443:192.168.1.101:443";
        assert!(ForwardRouteList::new(input).is_err());
    }

    #[test]
    fn test_to_string() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(
            route_list.to_string(),
            "8080:192.168.1.100:80, 8443:192.168.1.101:443"
        );
    }
}
