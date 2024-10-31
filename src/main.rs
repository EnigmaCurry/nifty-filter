use askama::Template;
use clap::Parser;
use dotenvy::from_filename;
use log::{error, info};
use parsers::port::PortList;
use std::collections::HashSet;
use std::env;
use std::process::exit;
mod format;
mod parsers;
use parsers::*;
#[allow(unused_imports)]
use std::net::IpAddr;

#[derive(Parser)]
#[command(name = "RouterConfig")]
#[command(about = "Generates router configuration from environment or .env file")]
struct Cli {
    /// Path to the .env file (actual environment vars supercede this)
    #[arg(long)]
    env_file: Option<String>,

    /// Ignore the environment (combine this with --env-file)
    #[arg(long)]
    strict_env: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Template)]
#[template(path = "router.nft.txt")]
struct RouterTemplate {
    interface_lan: Interface,
    interface_wan: Interface,
    icmp_accept_wan: String,
    icmp_accept_lan: String,
    subnet_lan: Subnet,
    tcp_accept_lan: String,
    udp_accept_lan: String,
    tcp_accept_wan: String,
    udp_accept_wan: String,
    tcp_forward_lan: ForwardRouteList,
    udp_forward_lan: ForwardRouteList,
    tcp_forward_wan: ForwardRouteList,
    udp_forward_wan: ForwardRouteList,
}

impl RouterTemplate {
    fn from_env() -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let interface_lan = get_interface("INTERFACE_LAN", &mut errors);
        let interface_wan = get_interface("INTERFACE_WAN", &mut errors);
        let subnet_lan = get_subnet("SUBNET_LAN", &mut errors);

        let icmp_accept_lan = IcmpType::vec_to_string(&get_icmp_types(
            "ICMP_ACCEPT_LAN",
            &mut errors,
            vec![
                IcmpType::EchoRequest,
                IcmpType::EchoReply,
                IcmpType::DestinationUnreachable,
                IcmpType::TimeExceeded,
            ],
        ));
        let icmp_accept_wan =
            IcmpType::vec_to_string(&get_icmp_types("ICMP_ACCEPT_WAN", &mut errors, vec![]));

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

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_lan,
            interface_wan,
            subnet_lan,
            icmp_accept_lan,
            icmp_accept_wan,
            tcp_accept_lan,
            udp_accept_lan,
            tcp_accept_wan,
            udp_accept_wan,
            tcp_forward_lan,
            udp_forward_lan,
            tcp_forward_wan,
            udp_forward_wan,
        })
    }
}

fn app() {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Set RUST_LOG to info if verbose is enabled
    if cli.verbose {
        env::set_var("RUST_LOG", "info");
    }

    // Initialize the logger
    env_logger::init();

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

    // Attempt to create the RouterTemplate from environment variables
    match RouterTemplate::from_env() {
        Ok(router) => println!("{}", format::reduce_blank_lines(&router.render().unwrap())),
        Err(errors) => {
            for err in errors {
                eprintln!("Error: {}", err);
            }
            std::process::exit(1);
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
