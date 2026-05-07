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
#[cfg(feature = "nixos")]
mod pve_setup;
pub mod vlan;
use parsers::*;
use vlan::Vlan;
#[allow(unused_imports)]
use std::net::IpAddr;

#[cfg(test)]
pub mod test_util {
    use std::sync::Mutex;
    /// Global lock for tests that manipulate environment variables.
    /// All env-mutating tests across the crate must hold this lock.
    pub static ENV_LOCK: Mutex<()> = Mutex::new(());
}
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

    /// Interactive PVE VM setup wizard (outputs shell variables)
    #[cfg(feature = "nixos")]
    PveSetup {
        /// PVE host to connect to
        pve_host: String,
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
    interface_trunk: Interface,
    interface_wan: Interface,
    interface_mgmt: String,
    subnet_mgmt_ipv4: String,

    // Protocol enablement
    enable_ipv4: bool,
    enable_ipv6: bool,

    // VLAN configuration
    vlan_aware_switch: bool,
    vlans: Vec<Vlan>,

    // WAN-side ICMP
    icmp_accept_wan: String,
    icmpv6_accept_wan: String,

    // WAN-side port accepts
    tcp_accept_wan: String,
    udp_accept_wan: String,

    // WAN-side forward routes
    tcp_forward_wan: ForwardRouteList,
    udp_forward_wan: ForwardRouteList,

    // iperf3 server port
    iperf_port: u16,

    // Anti-spoofing: bogon source addresses to drop on WAN
    wan_bogons_ipv4: String,
    wan_bogons_ipv6: String,
}

impl RouterTemplate {
    fn from_env() -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();

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

        // WAN protocol enablement
        // Accepts WAN_ENABLE_IPV4 or legacy ENABLE_IPV4 (same for IPv6)
        let enable_ipv4 = if env::var("WAN_ENABLE_IPV4").is_ok() {
            get_bool("WAN_ENABLE_IPV4", &mut errors, Some(true))
        } else {
            get_bool("ENABLE_IPV4", &mut errors, Some(true))
        };
        let enable_ipv6 = if env::var("WAN_ENABLE_IPV6").is_ok() {
            get_bool("WAN_ENABLE_IPV6", &mut errors, Some(false))
        } else {
            get_bool("ENABLE_IPV6", &mut errors, Some(false))
        };

        if !enable_ipv4 && !enable_ipv6 {
            errors.push("At least one of WAN_ENABLE_IPV4 or WAN_ENABLE_IPV6 must be true.".to_string());
        }

        // Parse VLANs (handles backward compat with legacy INTERFACE_LAN/SUBNET_LAN vars)
        let (trunk_name, vlan_aware_switch, vlans) =
            vlan::parse_vlans_from_env(enable_ipv4, &mut errors);

        let interface_trunk = match Interface::new(&trunk_name) {
            Ok(iface) => iface,
            Err(err) => {
                errors.push(err);
                Interface::new("eth0").unwrap()
            }
        };

        // WAN-side ICMP
        let icmp_accept_wan = if enable_ipv4 {
            IcmpType::vec_to_string(&get_icmp_types("ICMP_ACCEPT_WAN", &mut errors, vec![]))
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
                    Icmpv6Type::NdRouterSolicit,
                    Icmpv6Type::NdRouterAdvert,
                    Icmpv6Type::DestinationUnreachable,
                    Icmpv6Type::PacketTooBig,
                    Icmpv6Type::TimeExceeded,
                ],
            ))
        } else {
            String::new()
        };

        // WAN-side port accepts
        let tcp_accept_wan =
            get_port_accept("TCP_ACCEPT_WAN", &mut errors, PortList::new("").unwrap()).to_string();
        let udp_accept_wan =
            get_port_accept("UDP_ACCEPT_WAN", &mut errors, PortList::new("").unwrap()).to_string();

        // WAN-side forward routes
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

        // iperf3 port
        let iperf_port: u16 = env::var("IPERF_PORT")
            .unwrap_or_else(|_| "5201".to_string())
            .parse()
            .unwrap_or_else(|_| {
                errors.push("IPERF_PORT must be a valid port number.".to_string());
                5201
            });

        // Anti-spoofing bogon lists
        let wan_bogons_ipv4 = if enable_ipv4 {
            get_cidr_list(
                "WAN_BOGONS_IPV4",
                &mut errors,
                CidrList::new("0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24, 224.0.0.0/4, 240.0.0.0/4").unwrap(),
            )
            .to_string()
        } else {
            String::new()
        };
        let wan_bogons_ipv6 = if enable_ipv6 {
            get_cidr_list(
                "WAN_BOGONS_IPV6",
                &mut errors,
                CidrList::new("::/128, ::1/128, fc00::/7, ff00::/8").unwrap(),
            )
            .to_string()
        } else {
            String::new()
        };

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_trunk,
            interface_wan,
            interface_mgmt,
            subnet_mgmt_ipv4,
            enable_ipv4,
            enable_ipv6,
            vlan_aware_switch,
            vlans,
            icmp_accept_wan,
            icmpv6_accept_wan,
            tcp_accept_wan,
            udp_accept_wan,
            tcp_forward_wan,
            udp_forward_wan,
            iperf_port,
            wan_bogons_ipv4,
            wan_bogons_ipv6,
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
        #[cfg(feature = "nixos")]
        Commands::PveSetup { pve_host } => pve_setup::run(&pve_host),
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
    use askama::Template;
    use crate::test_util::ENV_LOCK;

    /// Clear all VLAN-related env vars to avoid test pollution.
    fn clear_env() {
        let keys_to_remove: Vec<String> = env::vars()
            .map(|(k, _)| k)
            .filter(|k| {
                k.starts_with("VLAN_")
                    || k.starts_with("INTERFACE_")
                    || k.starts_with("SUBNET_")
                    || k.starts_with("ENABLE_")
                    || k.starts_with("ICMP_")
                    || k.starts_with("ICMPV6_")
                    || k.starts_with("TCP_")
                    || k.starts_with("UDP_")
                    || k.starts_with("LAN_")
                    || k.starts_with("WAN_")
                    || k.starts_with("DHCP")
                    || k.starts_with("IPERF")
                    || k == "VLANS"
                    || k == "VLAN_AWARE_SWITCH"
            })
            .collect();
        for k in keys_to_remove {
            env::remove_var(&k);
        }
    }

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

    #[test]
    fn test_simple_mode_vlan1() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        env::set_var("INTERFACE_TRUNK", "trunk");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("VLAN_1_SUBNET_IPV4", "192.168.10.1/24");
        env::set_var("VLAN_1_TCP_ACCEPT", "22");
        env::set_var("VLAN_1_UDP_ACCEPT", "67,68");

        let tmpl = RouterTemplate::from_env().unwrap_or_else(|e| panic!("should parse simple mode: {:?}", e));
        let rendered = tmpl.render().unwrap();

        // Should reference trunk directly (VLAN 1 = bare trunk)
        assert!(rendered.contains(r#"iif "trunk""#));
        // Should NOT contain trunk.1 sub-interface
        assert!(!rendered.contains("trunk.1"));
        // Should have VLAN 1 egress rule (default allow-all)
        assert!(rendered.contains("ip saddr 192.168.10.1/24 ip daddr { 0.0.0.0/0 }"));
        // Should have SSH accept
        assert!(rendered.contains("tcp dport { 22 }"));
        // Should NOT have untagged trunk drop (not switch-aware)
        assert!(!rendered.contains("Dropped untagged trunk"));
        // Should NOT have inter-VLAN drop (only one VLAN)
        assert!(!rendered.contains("inter-VLAN"));

        clear_env();
    }

    #[test]
    fn test_vlan_aware_multi_vlan() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        env::set_var("INTERFACE_TRUNK", "trunk");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("VLAN_AWARE_SWITCH", "true");
        env::set_var("VLANS", "10,20");

        env::set_var("VLAN_10_SUBNET_IPV4", "10.10.0.1/24");
        env::set_var("VLAN_10_EGRESS_ALLOWED_IPV4", "0.0.0.0/0");
        env::set_var("VLAN_10_TCP_ACCEPT", "22");
        env::set_var("VLAN_10_UDP_ACCEPT", "67,68");

        env::set_var("VLAN_20_SUBNET_IPV4", "10.20.0.1/24");
        // No egress for VLAN 20 (IoT jail)
        env::set_var("VLAN_20_TCP_ACCEPT", "");
        env::set_var("VLAN_20_UDP_ACCEPT", "67,68");

        let tmpl = RouterTemplate::from_env().unwrap_or_else(|e| panic!("should parse multi-VLAN mode: {:?}", e));
        let rendered = tmpl.render().unwrap();

        // Should have VLAN sub-interfaces
        assert!(rendered.contains(r#"iif "trunk.10""#));
        assert!(rendered.contains(r#"iif "trunk.20""#));
        // VLAN 10 should have egress
        assert!(rendered.contains("ip saddr 10.10.0.1/24 ip daddr { 0.0.0.0/0 }"));
        // VLAN 20 should NOT have egress (empty)
        assert!(!rendered.contains("ip saddr 10.20.0.1/24 ip daddr"));
        // VLAN 10 should have SSH
        assert!(rendered.contains(r#"ip saddr 10.10.0.1/24 iif "trunk.10" tcp dport { 22 }"#));
        // VLAN 20 should NOT have TCP accept (empty)
        assert!(!rendered.contains(r#"iif "trunk.20" tcp dport"#));
        // Should have untagged trunk drop (switch-aware)
        assert!(rendered.contains("Dropped untagged trunk input"));
        assert!(rendered.contains("Dropped untagged trunk forward"));
        // Should have inter-VLAN drops
        assert!(rendered.contains("inter-VLAN"));
        assert!(rendered.contains(r#"iif "trunk.10" oif "trunk.20""#));
        assert!(rendered.contains(r#"iif "trunk.20" oif "trunk.10""#));

        clear_env();
    }

    #[test]
    fn test_vlan_aware_rejects_vlan_1() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        env::set_var("INTERFACE_TRUNK", "trunk");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("VLAN_AWARE_SWITCH", "true");
        env::set_var("VLANS", "1,10");
        env::set_var("VLAN_1_SUBNET_IPV4", "192.168.1.1/24");
        env::set_var("VLAN_10_SUBNET_IPV4", "10.10.0.1/24");

        let result = RouterTemplate::from_env();
        assert!(result.is_err());
        let errors = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(errors.iter().any(|e| e.contains("VLAN 1") && e.contains("not allowed")));

        clear_env();
    }

    #[test]
    fn test_backward_compat_legacy_vars() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        // Set legacy flat vars (no VLAN_* vars)
        env::set_var("INTERFACE_LAN", "lan");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("SUBNET_LAN", "192.168.1.1/24");
        env::set_var("TCP_ACCEPT_LAN", "22,80");
        env::set_var("UDP_ACCEPT_LAN", "67,68");
        env::set_var("LAN_EGRESS_ALLOWED_IPV4", "10.0.0.0/8");

        let tmpl = RouterTemplate::from_env().unwrap_or_else(|e| panic!("should parse legacy vars: {:?}", e));
        let rendered = tmpl.render().unwrap();

        // Should use "lan" as trunk name (from INTERFACE_LAN alias)
        assert!(rendered.contains(r#"iif "lan""#));
        // Should have the legacy subnet
        assert!(rendered.contains("192.168.1.1/24"));
        // Should have the legacy egress
        assert!(rendered.contains("10.0.0.0/8"));
        // Should have the legacy port accepts
        assert!(rendered.contains("tcp dport { 22, 80 }"));

        clear_env();
    }

    #[test]
    fn test_vlan_custom_names() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        env::set_var("INTERFACE_TRUNK", "trunk");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("VLAN_AWARE_SWITCH", "true");
        env::set_var("VLANS", "10,20");

        env::set_var("VLAN_10_NAME", "trusted");
        env::set_var("VLAN_10_SUBNET_IPV4", "10.10.0.1/24");
        env::set_var("VLAN_10_EGRESS_ALLOWED_IPV4", "0.0.0.0/0");
        env::set_var("VLAN_10_TCP_ACCEPT", "22");
        env::set_var("VLAN_10_UDP_ACCEPT", "67,68");

        env::set_var("VLAN_20_NAME", "iot");
        env::set_var("VLAN_20_SUBNET_IPV4", "10.20.0.1/24");
        env::set_var("VLAN_20_TCP_ACCEPT", "");
        env::set_var("VLAN_20_UDP_ACCEPT", "67,68");

        let tmpl = RouterTemplate::from_env().unwrap_or_else(|e| panic!("should parse custom names: {:?}", e));
        let rendered = tmpl.render().unwrap();

        // Should use custom names instead of trunk.N
        assert!(rendered.contains(r#"iif "trusted""#));
        assert!(rendered.contains(r#"iif "iot""#));
        assert!(!rendered.contains("trunk.10"));
        assert!(!rendered.contains("trunk.20"));
        // Inter-VLAN isolation uses custom names
        assert!(rendered.contains(r#"iif "trusted" oif "iot""#));
        assert!(rendered.contains(r#"iif "iot" oif "trusted""#));

        clear_env();
    }

    #[test]
    fn test_per_vlan_router_access_isolation() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env();

        env::set_var("INTERFACE_TRUNK", "trunk");
        env::set_var("INTERFACE_WAN", "wan");
        env::set_var("VLAN_AWARE_SWITCH", "true");
        env::set_var("VLANS", "10,20");

        // VLAN 10: trusted — SSH + DHCP + full ICMP
        env::set_var("VLAN_10_SUBNET_IPV4", "10.10.0.1/24");
        env::set_var("VLAN_10_TCP_ACCEPT", "22");
        env::set_var("VLAN_10_UDP_ACCEPT", "67,68");
        env::set_var("VLAN_10_ICMP_ACCEPT", "echo-request,echo-reply,destination-unreachable,time-exceeded");

        // VLAN 20: IoT — DHCP only, minimal ICMP
        env::set_var("VLAN_20_SUBNET_IPV4", "10.20.0.1/24");
        env::set_var("VLAN_20_TCP_ACCEPT", "");
        env::set_var("VLAN_20_UDP_ACCEPT", "67,68");
        env::set_var("VLAN_20_ICMP_ACCEPT", "destination-unreachable");

        let tmpl = RouterTemplate::from_env().unwrap_or_else(|e| panic!("should parse: {:?}", e));
        let rendered = tmpl.render().unwrap();

        // VLAN 10 gets SSH
        assert!(rendered.contains(r#"ip saddr 10.10.0.1/24 iif "trunk.10" tcp dport { 22 }"#));
        // VLAN 20 does NOT get any TCP accept (empty string, block skipped)
        // Verify no tcp dport rule for trunk.20
        assert!(!rendered.contains(r#"iif "trunk.20" tcp dport"#));
        // VLAN 10 gets full ICMP
        assert!(rendered.contains(r#"iif "trunk.10" ip saddr 10.10.0.1/24 icmp type { echo-request, echo-reply, destination-unreachable, time-exceeded }"#));
        // VLAN 20 gets minimal ICMP
        assert!(rendered.contains(r#"iif "trunk.20" ip saddr 10.20.0.1/24 icmp type { destination-unreachable }"#));

        clear_env();
    }
}
