use std::process::Command;

use inquire::{InquireError, Select, Text};
use ipnetwork::IpNetwork;
use regex::Regex;

use super::env_file::EnvFile;

const ENV_FILE: &str = "/var/nifty-filter/router.env";
const DHCP_FILE: &str = "/var/nifty-filter/dhcp.env";

fn prompt_text(message: &str, default: &str) -> Option<String> {
    let mut prompt = Text::new(message);
    if !default.is_empty() {
        prompt = prompt.with_default(default);
    }
    match prompt.prompt() {
        Ok(val) => Some(val),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => None,
        Err(_) => None,
    }
}

fn prompt_text_allow_blank(message: &str, default: &str) -> Option<String> {
    let prompt = Text::new(message).with_default(default);
    match prompt.prompt() {
        Ok(val) => Some(val),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => None,
        Err(_) => None,
    }
}

fn choose(message: &str, options: Vec<String>, cursor: usize) -> Option<(usize, String)> {
    let cursor = cursor.min(options.len().saturating_sub(1));
    match Select::new(message, options)
        .with_starting_cursor(cursor)
        .raw_prompt()
    {
        Ok(choice) => Some((choice.index, choice.value)),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => None,
        Err(_) => None,
    }
}

// --- Editor functions ---

fn edit_hostname(router: &mut EnvFile) {
    let current = router.get("HOSTNAME").to_string();
    let val = match prompt_text("Hostname", &current) {
        Some(v) => v,
        None => return,
    };
    let re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();
    if re.is_match(&val) {
        router.set("HOSTNAME", &val);
        router.save().ok();
        println!("  Set HOSTNAME={val}");
    } else {
        println!("  Invalid hostname.");
    }
}

fn edit_subnet(router: &mut EnvFile, dhcp: &mut EnvFile) {
    let current = router.get("SUBNET_LAN").to_string();
    let default = if current.is_empty() {
        "10.99.0.1/24".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text("LAN subnet (IP/prefix)", &default) {
            Some(v) => v,
            None => return,
        };
        if v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).");
    };
    router.set("SUBNET_LAN", &val);
    router.save().ok();
    println!("  Set SUBNET_LAN={val}");

    // Update DHCP defaults to match
    if let Some((router_ip, _)) = val.split_once('/') {
        if let Some(base) = router_ip.rsplit_once('.') {
            dhcp.set("DHCP_SUBNET", &val);
            dhcp.set("DHCP_ROUTER", router_ip);
            dhcp.set("DHCP_POOL_START", &format!("{}.100", base.0));
            dhcp.set("DHCP_POOL_END", &format!("{}.250", base.0));
            dhcp.save().ok();
            println!("  Updated DHCP pool to match.");
        }
    }
}

fn edit_subnet_ipv6(router: &mut EnvFile) {
    let current = router.get("SUBNET_LAN_IPV6").to_string();
    let default = if current.is_empty() {
        "fd00:10::1/64".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text("LAN IPv6 subnet (IP/prefix)", &default) {
            Some(v) => v,
            None => return,
        };
        if v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. fd00:10::1/64).");
    };
    router.set("SUBNET_LAN_IPV6", &val);
    router.save().ok();
    println!("  Set SUBNET_LAN_IPV6={val}");
}

fn toggle_ipv6(router: &mut EnvFile) {
    if router.get("ENABLE_IPV6") == "true" {
        router.set("ENABLE_IPV6", "false");
        router.save().ok();
        println!("  IPv6 disabled.");
    } else {
        router.set("ENABLE_IPV6", "true");
        router.save().ok();
        if router.get("SUBNET_LAN_IPV6").is_empty() {
            println!("  IPv6 requires a LAN subnet.");
            edit_subnet_ipv6(router);
        }
        println!("  IPv6 enabled.");
    }
}

fn validate_cidr_list(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    input
        .split(',')
        .all(|s| s.trim().parse::<IpNetwork>().is_ok())
}

fn edit_egress_ipv4(router: &mut EnvFile) {
    let current = router.get("LAN_EGRESS_ALLOWED_IPV4").to_string();
    let default = if current.is_empty() {
        "0.0.0.0/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text("Allowed IPv4 egress CIDRs (comma-separated)", &default) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like 0.0.0.0/0 or 10.0.0.0/8,172.16.0.0/12.");
    };
    router.set("LAN_EGRESS_ALLOWED_IPV4", &val);
    router.save().ok();
    println!("  Set LAN_EGRESS_ALLOWED_IPV4={val}");
}

fn edit_egress_ipv6(router: &mut EnvFile) {
    let current = router.get("LAN_EGRESS_ALLOWED_IPV6").to_string();
    let default = if current.is_empty() {
        "::/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text("Allowed IPv6 egress CIDRs (comma-separated)", &default) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like ::/0 or fd00::/8.");
    };
    router.set("LAN_EGRESS_ALLOWED_IPV6", &val);
    router.save().ok();
    println!("  Set LAN_EGRESS_ALLOWED_IPV6={val}");
}

fn edit_dhcp_pool(dhcp: &mut EnvFile) {
    let start = dhcp.get("DHCP_POOL_START").to_string();
    let end = dhcp.get("DHCP_POOL_END").to_string();
    let start = match prompt_text("DHCP pool start", &start) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text("DHCP pool end", &end) {
        Some(v) => v,
        None => return,
    };
    dhcp.set("DHCP_POOL_START", &start);
    dhcp.set("DHCP_POOL_END", &end);
    dhcp.save().ok();
    println!("  Set pool: {start} - {end}");
}

fn edit_dns(dhcp: &mut EnvFile) {
    let current = dhcp.get("DHCP_DNS").to_string();
    let val = match prompt_text("DNS servers (comma-separated)", &current) {
        Some(v) => v,
        None => return,
    };
    dhcp.set("DHCP_DNS", &val);
    dhcp.save().ok();
    println!("  Set DNS={val}");
}

fn toggle_dhcpv6(router: &mut EnvFile, dhcp: &mut EnvFile) {
    if dhcp.get("DHCPV6_ENABLED") == "true" {
        dhcp.set("DHCPV6_ENABLED", "false");
        dhcp.save().ok();
        // Remove DHCPv6 ports from UDP_ACCEPT_LAN
        let udp_lan = router.get("UDP_ACCEPT_LAN").to_string();
        let cleaned = udp_lan
            .replace(",546,547", "")
            .replace("546,547,", "")
            .replace("546,547", "");
        router.set("UDP_ACCEPT_LAN", &cleaned);
        router.save().ok();
        println!("  DHCPv6 disabled. Removed ports 546,547 from UDP_ACCEPT_LAN.");
    } else {
        dhcp.set("DHCPV6_ENABLED", "true");
        dhcp.save().ok();
        // Add DHCPv6 ports to UDP_ACCEPT_LAN if not already present
        let udp_lan = router.get("UDP_ACCEPT_LAN").to_string();
        if !udp_lan.contains("546") {
            let new_val = if udp_lan.is_empty() {
                "546,547".to_string()
            } else {
                format!("{udp_lan},546,547")
            };
            router.set("UDP_ACCEPT_LAN", &new_val);
            router.save().ok();
            println!("  Added ports 546,547 to UDP_ACCEPT_LAN.");
        }
        if dhcp.get("DHCPV6_POOL_START").is_empty() {
            println!("  DHCPv6 requires a pool range.");
            edit_dhcpv6_pool(router, dhcp);
        }
        println!("  DHCPv6 enabled.");
    }
}

fn edit_dhcpv6_pool(router: &EnvFile, dhcp: &mut EnvFile) {
    let mut start = dhcp.get("DHCPV6_POOL_START").to_string();
    let mut end = dhcp.get("DHCPV6_POOL_END").to_string();
    // Derive defaults from the IPv6 subnet if pool is not yet set
    if start.is_empty() {
        let subnet = router.get("SUBNET_LAN_IPV6");
        if let Some((addr, _)) = subnet.split_once('/') {
            if let Some(prefix) = addr.rsplit_once(':') {
                start = format!("{}:100", prefix.0);
                end = format!("{}:1ff", prefix.0);
            }
        }
    }
    let start = match prompt_text("DHCPv6 pool start", &start) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text("DHCPv6 pool end", &end) {
        Some(v) => v,
        None => return,
    };
    dhcp.set("DHCPV6_POOL_START", &start);
    dhcp.set("DHCPV6_POOL_END", &end);
    dhcp.save().ok();
    println!("  Set DHCPv6 pool: {start} - {end}");
}

fn edit_ports(router: &mut EnvFile, key: &str, label: &str) {
    let current = router.get(key).to_string();
    let val = match prompt_text_allow_blank(&format!("{label} (comma-separated)"), &current) {
        Some(v) => v,
        None => return,
    };
    router.set(key, &val);
    router.save().ok();
    println!("  Set {key}={val}");
}

fn edit_forwards(router: &mut EnvFile, key: &str, label: &str) {
    let current = router.get(key).to_string();
    println!("  Format: incoming_port:dest_ip:dest_port (comma-separated)");
    println!("  IPv6:   incoming_port:[ipv6_addr]:dest_port");
    let val = match prompt_text_allow_blank(label, &current) {
        Some(v) => v,
        None => return,
    };
    router.set(key, &val);
    router.save().ok();
    println!("  Set {key}={val}");
}

fn toggle_enabled(router: &mut EnvFile) {
    if router.get("ENABLED") == "true" {
        router.set("ENABLED", "false");
        router.save().ok();
        println!("  Disabled.");
    } else {
        router.set("ENABLED", "true");
        router.save().ok();
        println!("  Enabled.");
    }
}

fn apply_changes(router: &EnvFile) {
    for (service, label) in [
        ("nifty-filter", "Firewall rules"),
        ("nifty-network", "Network"),
        ("nifty-dnsmasq", "DHCP/DNS"),
    ] {
        println!("  Restarting {service}...");
        match Command::new("sudo")
            .args(["systemctl", "restart", service])
            .status()
        {
            Ok(s) if s.success() => println!("  {label} applied."),
            _ => println!("  Failed! Check: journalctl -u {service}"),
        }
    }
    println!("  Setting hostname...");
    let hostname = router.get("HOSTNAME");
    let _ = Command::new("sudo")
        .args(["hostname", hostname])
        .status();
    println!("  Done.");
}

fn show_status(router: &EnvFile, dhcp: &EnvFile) {
    println!();
    println!("  === Current Configuration ===");
    println!("  ENABLED:        {}", router.get("ENABLED"));
    println!("  HOSTNAME:       {}", router.get("HOSTNAME"));
    println!("  INTERFACE_WAN:  {}", router.get("INTERFACE_WAN"));
    println!("  INTERFACE_LAN:  {}", router.get("INTERFACE_LAN"));
    println!("  ENABLE_IPV4:    {}", router.get("ENABLE_IPV4"));
    println!("  ENABLE_IPV6:    {}", router.get("ENABLE_IPV6"));
    println!("  SUBNET_LAN:     {}", router.get("SUBNET_LAN"));
    let ipv6_subnet = router.get("SUBNET_LAN_IPV6");
    if !ipv6_subnet.is_empty() {
        println!("  SUBNET_LAN_IPV6: {ipv6_subnet}");
    }
    println!("  TCP_ACCEPT_LAN: {}", router.get("TCP_ACCEPT_LAN"));
    println!("  UDP_ACCEPT_LAN: {}", router.get("UDP_ACCEPT_LAN"));
    println!("  TCP_ACCEPT_WAN: {}", router.get("TCP_ACCEPT_WAN"));
    println!("  UDP_ACCEPT_WAN: {}", router.get("UDP_ACCEPT_WAN"));
    let egress4 = router.get("LAN_EGRESS_ALLOWED_IPV4");
    if !egress4.is_empty() {
        println!("  EGRESS_IPV4:    {egress4}");
    }
    let egress6 = router.get("LAN_EGRESS_ALLOWED_IPV6");
    if !egress6.is_empty() {
        println!("  EGRESS_IPV6:    {egress6}");
    }
    println!("  TCP_FORWARD_LAN: {}", router.get("TCP_FORWARD_LAN"));
    println!("  UDP_FORWARD_LAN: {}", router.get("UDP_FORWARD_LAN"));
    println!("  TCP_FORWARD_WAN: {}", router.get("TCP_FORWARD_WAN"));
    println!("  UDP_FORWARD_WAN: {}", router.get("UDP_FORWARD_WAN"));
    println!(
        "  DHCP_POOL:      {} - {}",
        dhcp.get("DHCP_POOL_START"),
        dhcp.get("DHCP_POOL_END")
    );
    println!("  DHCP_DNS:       {}", dhcp.get("DHCP_DNS"));
    if dhcp.get("DHCPV6_ENABLED") == "true" {
        println!(
            "  DHCPV6:         {} - {}",
            dhcp.get("DHCPV6_POOL_START"),
            dhcp.get("DHCPV6_POOL_END")
        );
    }
    println!();
}

fn launch_editor(path: &str) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    let _ = Command::new(&editor).arg(path).status();
}

// --- Submenus ---

fn menu_network(router: &mut EnvFile, dhcp: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = router.get("ENABLE_IPV6") == "true";
        let ipv6_label = if ipv6_enabled {
            "Disable IPv6"
        } else {
            "Enable IPv6"
        };

        let mut items = vec![
            format!("Hostname ({})", router.get("HOSTNAME")),
            format!("LAN IPv4 subnet ({})", router.get("SUBNET_LAN")),
        ];
        if ipv6_enabled {
            items.push(format!(
                "LAN IPv6 subnet ({})",
                router.get("SUBNET_LAN_IPV6")
            ));
        }
        items.push(ipv6_label.to_string());
        items.push("Back".to_string());

        match choose("Network:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("Hostname") => { cursor = idx; edit_hostname(router) }
            Some((idx, choice)) if choice.starts_with("LAN IPv4 subnet") => { cursor = idx; edit_subnet(router, dhcp) }
            Some((idx, choice)) if choice.starts_with("LAN IPv6 subnet") => { cursor = idx; edit_subnet_ipv6(router) }
            Some((idx, choice)) if choice == "Enable IPv6" || choice == "Disable IPv6" => {
                cursor = idx; toggle_ipv6(router)
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_firewall(router: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = router.get("ENABLE_IPV6") == "true";

        let mut items = vec![
            format!("TCP ports LAN ({})", router.get("TCP_ACCEPT_LAN")),
            format!("UDP ports LAN ({})", router.get("UDP_ACCEPT_LAN")),
            format!("TCP ports WAN ({})", router.get("TCP_ACCEPT_WAN")),
            format!("UDP ports WAN ({})", router.get("UDP_ACCEPT_WAN")),
            format!(
                "Egress filter IPv4 ({})",
                router.get("LAN_EGRESS_ALLOWED_IPV4")
            ),
        ];
        if ipv6_enabled {
            items.push(format!(
                "Egress filter IPv6 ({})",
                router.get("LAN_EGRESS_ALLOWED_IPV6")
            ));
        }
        items.push("Back".to_string());

        match choose("Firewall:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("TCP ports LAN") => {
                cursor = idx; edit_ports(router, "TCP_ACCEPT_LAN", "TCP ports LAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP ports LAN") => {
                cursor = idx; edit_ports(router, "UDP_ACCEPT_LAN", "UDP ports LAN")
            }
            Some((idx, choice)) if choice.starts_with("TCP ports WAN") => {
                cursor = idx; edit_ports(router, "TCP_ACCEPT_WAN", "TCP ports WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP ports WAN") => {
                cursor = idx; edit_ports(router, "UDP_ACCEPT_WAN", "UDP ports WAN")
            }
            Some((idx, choice)) if choice.starts_with("Egress filter IPv4") => { cursor = idx; edit_egress_ipv4(router) }
            Some((idx, choice)) if choice.starts_with("Egress filter IPv6") => { cursor = idx; edit_egress_ipv6(router) }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_port_forwarding(router: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let items = vec![
            format!("TCP forward LAN ({})", router.get("TCP_FORWARD_LAN")),
            format!("UDP forward LAN ({})", router.get("UDP_FORWARD_LAN")),
            format!("TCP forward WAN ({})", router.get("TCP_FORWARD_WAN")),
            format!("UDP forward WAN ({})", router.get("UDP_FORWARD_WAN")),
            "Back".to_string(),
        ];

        match choose("Port Forwarding:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("TCP forward LAN") => {
                cursor = idx; edit_forwards(router, "TCP_FORWARD_LAN", "TCP forward LAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP forward LAN") => {
                cursor = idx; edit_forwards(router, "UDP_FORWARD_LAN", "UDP forward LAN")
            }
            Some((idx, choice)) if choice.starts_with("TCP forward WAN") => {
                cursor = idx; edit_forwards(router, "TCP_FORWARD_WAN", "TCP forward WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP forward WAN") => {
                cursor = idx; edit_forwards(router, "UDP_FORWARD_WAN", "UDP forward WAN")
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_dhcp_dns(router: &mut EnvFile, dhcp: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = router.get("ENABLE_IPV6") == "true";
        let dhcpv6_enabled = dhcp.get("DHCPV6_ENABLED") == "true";

        let mut items = vec![
            format!(
                "DHCP pool ({} - {})",
                dhcp.get("DHCP_POOL_START"),
                dhcp.get("DHCP_POOL_END")
            ),
            format!("DNS servers ({})", dhcp.get("DHCP_DNS")),
        ];
        if ipv6_enabled {
            let dhcpv6_label = if dhcpv6_enabled {
                "Disable DHCPv6"
            } else {
                "Enable DHCPv6"
            };
            items.push(dhcpv6_label.to_string());
            if dhcpv6_enabled {
                items.push(format!(
                    "DHCPv6 pool ({} - {})",
                    dhcp.get("DHCPV6_POOL_START"),
                    dhcp.get("DHCPV6_POOL_END")
                ));
            }
        }
        items.push("Back".to_string());

        match choose("DHCP / DNS:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("DHCP pool") => { cursor = idx; edit_dhcp_pool(dhcp) }
            Some((idx, choice)) if choice.starts_with("DNS servers") => { cursor = idx; edit_dns(dhcp) }
            Some((idx, choice)) if choice == "Enable DHCPv6" || choice == "Disable DHCPv6" => {
                cursor = idx; toggle_dhcpv6(router, dhcp)
            }
            Some((idx, choice)) if choice.starts_with("DHCPv6 pool") => { cursor = idx; edit_dhcpv6_pool(router, dhcp) }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

// --- Main menu ---

pub fn run() {
    let mut router = match EnvFile::load(std::path::Path::new(ENV_FILE)) {
        Ok(env) => env,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };
    let mut dhcp = match EnvFile::load(std::path::Path::new(DHCP_FILE)) {
        Ok(env) => env,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };

    let mut cursor = 0;
    loop {
        println!();
        let enabled_label = if router.get("ENABLED") == "true" {
            "Disable firewall"
        } else {
            "Enable firewall"
        };

        let items = vec![
            "Show config".to_string(),
            "Network".to_string(),
            "Firewall".to_string(),
            "Port forwarding".to_string(),
            "DHCP / DNS".to_string(),
            enabled_label.to_string(),
            "Apply changes".to_string(),
            "Edit router.env".to_string(),
            "Edit dhcp.env".to_string(),
            "Quit".to_string(),
        ];

        match choose("nifty-filter configuration:", items, cursor) {
            Some((idx, choice)) => { cursor = idx; match choice.as_str() {
                "Show config" => show_status(&router, &dhcp),
                "Network" => menu_network(&mut router, &mut dhcp),
                "Firewall" => menu_firewall(&mut router),
                "Port forwarding" => menu_port_forwarding(&mut router),
                "DHCP / DNS" => menu_dhcp_dns(&mut router, &mut dhcp),
                "Enable firewall" | "Disable firewall" => toggle_enabled(&mut router),
                "Apply changes" => apply_changes(&router),
                "Edit router.env" => {
                    launch_editor(ENV_FILE);
                    if let Err(e) = router.reload() {
                        eprintln!("  Warning: {e}");
                    }
                }
                "Edit dhcp.env" => {
                    launch_editor(DHCP_FILE);
                    if let Err(e) = dhcp.reload() {
                        eprintln!("  Warning: {e}");
                    }
                }
                "Quit" => break,
                _ => {}
            }},
            None => break, // ESC at main menu = quit
        }
    }
}
