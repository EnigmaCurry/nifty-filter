use askama::Template;
use clap::Parser;
use dotenvy::from_filename;
use env_logger;
use log::{error, info};
use std::collections::HashSet;
use std::env;
use std::process::exit;
mod parsers;
use parsers::*;

#[derive(Parser)]
#[command(name = "RouterConfig")]
#[command(about = "Generates router configuration from environment or .env file")]
struct Cli {
    /// Path to the .env file (actual environment vars supercede this)
    #[arg(long)]
    env_file: Option<String>,

    /// Ignore the environment (combine this with --env-file)
    #[arg(long)]
    ignore_env: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Template)]
#[template(path = "router.nft.txt")]
struct RouterTemplate {
    interface_lan: Interface,
    interface_wan: Interface,
    icmp_accept_lan: String,
    icmp_accept_wan: String,
    subnet_lan: Subnet,
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

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_lan,
            interface_wan,
            subnet_lan,
            icmp_accept_lan,
            icmp_accept_wan,
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

    // Ignore non-default environment variables if `--ignore-env` is set
    if cli.ignore_env {
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
        Ok(router) => {
            //
            println!("{}", router.render().unwrap())
        }
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
