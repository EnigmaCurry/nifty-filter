use askama::Template;
use clap::{Parser, Subcommand};
use dnsmasq::DnsmasqTemplate;
use dotenvy::from_filename;
use env_logger;
use log::{error, info};
use nftables::RouterTemplate;
use std::collections::HashSet;
use std::env;
#[allow(unused_imports)]
use std::net::IpAddr;
use std::process::exit;
use std::process::{self, Stdio};
use tui::main as config_main;

mod dnsmasq;
mod format;
mod info;
mod nftables;
mod parsers;
mod systemd;
mod systemd_network;
mod tui;

#[derive(Parser)]
#[command(name = "RouterConfig")]
#[command(about = "Generates router configuration from environment or .env file")]
struct Cli {
    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Path to the .env file (actual environment vars supersede this)
    #[arg(long)]
    env_file: Option<String>,

    /// Ignore the environment (combine this with --env-file)
    #[arg(long)]
    strict_env: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Config menu (default action)
    #[command()]
    Config {},
    /// Information commands
    #[command()]
    Info {
        #[command(subcommand)]
        info_command: InfoCommand,
    },
    /// Generate nftables configuration
    #[command(alias = "nft")]
    Nftables {
        /// Validate with nft -c (only works if interfaces exist on this host)
        #[arg(long)]
        validate: bool,
    },
    Dnsmasq {},
}

#[derive(Subcommand)]
enum InfoCommand {
    /// Print network interface information
    #[command()]
    Interfaces,
    Network,
}
pub fn validate_nftables_config(config: &str) -> Result<(), String> {
    let output = process::Command::new("nft")
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

fn app() {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Initialize the logger
    env_logger::init();

    // Set RUST_LOG to info if verbose is enabled
    if cli.verbose {
        env::set_var("RUST_LOG", "info");
    }

    // Ignore non-default environment variables if `--strict-env` is set
    if cli.env_file.is_some() && cli.strict_env {
        let default_vars: HashSet<&str> = [
            "HOME", "USER", "PWD", "OLDPWD", "SHELL", "PATH", "LANG", "TERM", "UID", "EUID",
            "LOGNAME", "HOSTNAME", "EDITOR", "VISUAL",
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
    if let Some(env_file) = cli.env_file {
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

    match cli.command.unwrap_or_else(|| Command::Config {}) {
        Command::Config {} => {
            config_main();
        }
        Command::Info { info_command } => match info_command {
            InfoCommand::Interfaces => {
                info::interfaces::interfaces().expect("failed to get interfaces info")
            }
            InfoCommand::Network => info::network::network().expect("failed to get network info"),
        },
        Command::Nftables { validate } => {
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
        Command::Dnsmasq {} => {
            // Attempt to create the DnsmasqTemplate from environment variables
            match DnsmasqTemplate::from_env() {
                Ok(config) => {
                    let text = format::reduce_blank_lines(&config.render().unwrap());
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
