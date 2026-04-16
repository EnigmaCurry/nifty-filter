use std::fs;
use std::process::Command;

use inquire::{InquireError, Select, Text};
use ipnetwork::IpNetwork;
use regex::Regex;

use super::env_file::EnvFile;

const ENV_FILE: &str = "/var/nifty-filter/nifty-filter.env";

/// Run a command in its own process group so Ctrl-C only kills the child.
fn run_interactive(cmd: &mut Command) {
    // Ignore SIGINT in parent while child runs
    unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN); }
    if let Ok(mut child) = cmd.spawn() {
        let pid = child.id() as libc::pid_t;
        // Put child in its own process group and make it the foreground group
        unsafe {
            libc::setpgid(pid, pid);
            libc::tcsetpgrp(0, pid);
        }
        let _ = child.wait();
        // Restore parent as foreground process group
        unsafe {
            libc::tcsetpgrp(0, libc::getpgrp());
        }
    }
    unsafe { libc::signal(libc::SIGINT, libc::SIG_DFL); }
}

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

fn edit_hostname(env: &mut EnvFile) {
    let current = env.get("HOSTNAME").to_string();
    let val = match prompt_text("Hostname", &current) {
        Some(v) => v,
        None => return,
    };
    let re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();
    if re.is_match(&val) {
        env.set("HOSTNAME", &val);
        env.save().ok();
        println!("  Set HOSTNAME={val}");
    } else {
        println!("  Invalid hostname.");
    }
}

fn edit_subnet(env: &mut EnvFile) {
    let current = env.get("SUBNET_LAN").to_string();
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
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).");
    };
    env.set("SUBNET_LAN", &val);

    // Update DHCP defaults to match
    if let Some((router_ip, _)) = val.split_once('/') {
        if let Some(base) = router_ip.rsplit_once('.') {
            env.set("DHCP_SUBNET", &val);
            env.set("DHCP_ROUTER", router_ip);
            env.set("DHCP_POOL_START", &format!("{}.100", base.0));
            env.set("DHCP_POOL_END", &format!("{}.250", base.0));
            println!("  Updated DHCP pool to match.");
        }
    }
    env.save().ok();
    println!("  Set SUBNET_LAN={val}");
}

fn edit_subnet_ipv6(env: &mut EnvFile) {
    let current = env.get("SUBNET_LAN_IPV6").to_string();
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
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. fd00:10::1/64).");
    };
    env.set("SUBNET_LAN_IPV6", &val);
    env.save().ok();
    println!("  Set SUBNET_LAN_IPV6={val}");
}

fn toggle_ipv6(env: &mut EnvFile) {
    if env.get("ENABLE_IPV6") == "true" {
        env.set("ENABLE_IPV6", "false");
        env.save().ok();
        println!("  IPv6 disabled.");
    } else {
        env.set("ENABLE_IPV6", "true");
        // Add IPv6 DNS servers if still using IPv4-only defaults
        let dns = env.get("DHCP_DNS").to_string();
        if dns == "1.1.1.1, 1.0.0.1" {
            env.set("DHCP_DNS", "1.1.1.1, 1.0.0.1, 2606:4700:4700::1111, 2606:4700:4700::1001");
            println!("  Added IPv6 DNS servers (Cloudflare).");
        }
        env.save().ok();
        if env.get("SUBNET_LAN_IPV6").is_empty() {
            println!("  IPv6 requires a LAN subnet.");
            edit_subnet_ipv6(env);
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

fn edit_egress_ipv4(env: &mut EnvFile) {
    let current = env.get("LAN_EGRESS_ALLOWED_IPV4").to_string();
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
    env.set("LAN_EGRESS_ALLOWED_IPV4", &val);
    env.save().ok();
    println!("  Set LAN_EGRESS_ALLOWED_IPV4={val}");
}

fn edit_egress_ipv6(env: &mut EnvFile) {
    let current = env.get("LAN_EGRESS_ALLOWED_IPV6").to_string();
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
    env.set("LAN_EGRESS_ALLOWED_IPV6", &val);
    env.save().ok();
    println!("  Set LAN_EGRESS_ALLOWED_IPV6={val}");
}

fn edit_dhcp_pool(env: &mut EnvFile) {
    let start = env.get("DHCP_POOL_START").to_string();
    let end = env.get("DHCP_POOL_END").to_string();
    let start = match prompt_text("DHCP pool start", &start) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text("DHCP pool end", &end) {
        Some(v) => v,
        None => return,
    };
    env.set("DHCP_POOL_START", &start);
    env.set("DHCP_POOL_END", &end);
    env.save().ok();
    println!("  Set pool: {start} - {end}");
}

fn edit_dns(env: &mut EnvFile) {
    let current = env.get("DHCP_DNS").to_string();
    let val = match prompt_text("DNS servers (comma-separated)", &current) {
        Some(v) => v,
        None => return,
    };
    env.set("DHCP_DNS", &val);
    env.save().ok();
    println!("  Set DNS={val}");
}

fn toggle_dhcp4(env: &mut EnvFile) {
    let enabled = env.get("DHCP4_ENABLED");
    let currently_enabled = enabled.is_empty() || enabled == "true";
    if currently_enabled {
        env.set("DHCP4_ENABLED", "false");
        // Remove DHCPv4 ports from UDP_ACCEPT_LAN
        let udp_lan = env.get("UDP_ACCEPT_LAN").to_string();
        let cleaned = udp_lan
            .replace(",67,68", "")
            .replace("67,68,", "")
            .replace("67,68", "");
        env.set("UDP_ACCEPT_LAN", &cleaned);
        env.save().ok();
        println!("  DHCPv4 disabled. Removed ports 67,68 from UDP_ACCEPT_LAN.");
    } else {
        env.set("DHCP4_ENABLED", "true");
        // Add DHCPv4 ports to UDP_ACCEPT_LAN if not already present
        let udp_lan = env.get("UDP_ACCEPT_LAN").to_string();
        if !udp_lan.contains("67") {
            let new_val = if udp_lan.is_empty() {
                "67,68".to_string()
            } else {
                format!("{udp_lan},67,68")
            };
            env.set("UDP_ACCEPT_LAN", &new_val);
        }
        env.save().ok();
        println!("  DHCPv4 enabled.");
    }
}

fn toggle_dhcpv6(env: &mut EnvFile) {
    if env.get("DHCPV6_ENABLED") == "true" {
        env.set("DHCPV6_ENABLED", "false");
        // Remove DHCPv6 ports from UDP_ACCEPT_LAN
        let udp_lan = env.get("UDP_ACCEPT_LAN").to_string();
        let cleaned = udp_lan
            .replace(",546,547", "")
            .replace("546,547,", "")
            .replace("546,547", "");
        env.set("UDP_ACCEPT_LAN", &cleaned);
        env.save().ok();
        println!("  DHCPv6 disabled. Removed ports 546,547 from UDP_ACCEPT_LAN.");
    } else {
        env.set("DHCPV6_ENABLED", "true");
        // Add IPv6 DNS servers if still using IPv4-only defaults
        let dns = env.get("DHCP_DNS").to_string();
        if dns == "1.1.1.1, 1.0.0.1" {
            env.set("DHCP_DNS", "1.1.1.1, 1.0.0.1, 2606:4700:4700::1111, 2606:4700:4700::1001");
            println!("  Added IPv6 DNS servers (Cloudflare).");
        }
        // Add DHCPv6 ports to UDP_ACCEPT_LAN if not already present
        let udp_lan = env.get("UDP_ACCEPT_LAN").to_string();
        if !udp_lan.contains("546") {
            let new_val = if udp_lan.is_empty() {
                "546,547".to_string()
            } else {
                format!("{udp_lan},546,547")
            };
            env.set("UDP_ACCEPT_LAN", &new_val);
        }
        env.save().ok();
        if env.get("DHCPV6_POOL_START").is_empty() {
            println!("  DHCPv6 requires a pool range.");
            edit_dhcpv6_pool(env);
        }
        println!("  DHCPv6 enabled.");
    }
}

fn edit_dhcpv6_pool(env: &mut EnvFile) {
    let mut start = env.get("DHCPV6_POOL_START").to_string();
    let mut end = env.get("DHCPV6_POOL_END").to_string();
    // Derive defaults from the IPv6 subnet if pool is not yet set
    if start.is_empty() {
        let subnet = env.get("SUBNET_LAN_IPV6");
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
    env.set("DHCPV6_POOL_START", &start);
    env.set("DHCPV6_POOL_END", &end);
    env.save().ok();
    println!("  Set DHCPv6 pool: {start} - {end}");
}

fn edit_ports(env: &mut EnvFile, key: &str, label: &str) {
    let current = env.get(key).to_string();
    let val = match prompt_text_allow_blank(&format!("{label} (comma-separated)"), &current) {
        Some(v) => v,
        None => return,
    };
    env.set(key, &val);
    env.save().ok();
    println!("  Set {key}={val}");
}

fn edit_forwards(env: &mut EnvFile, key: &str, label: &str) {
    let current = env.get(key).to_string();
    println!("  Format: incoming_port:dest_ip:dest_port (comma-separated)");
    println!("  IPv6:   incoming_port:[ipv6_addr]:dest_port");
    let val = match prompt_text_allow_blank(label, &current) {
        Some(v) => v,
        None => return,
    };
    env.set(key, &val);
    env.save().ok();
    println!("  Set {key}={val}");
}

fn toggle_enabled(env: &mut EnvFile) {
    if env.get("ENABLED") == "true" {
        env.set("ENABLED", "false");
        env.save().ok();
        println!("  Disabled.");
    } else {
        env.set("ENABLED", "true");
        env.save().ok();
        println!("  Enabled.");
    }
}

fn apply_changes(env: &EnvFile) {
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
    let hostname = env.get("HOSTNAME");
    let _ = Command::new("sudo")
        .args(["hostname", hostname])
        .status();
    println!("  Done.");
}

fn show_status() {
    println!();
    println!("  === Service Status ===");
    let services = [
        ("nifty-filter", "Firewall"),
        ("nifty-network", "Network"),
        ("nifty-dnsmasq", "DHCP / DNS"),
        ("nifty-hostname", "Hostname"),
        ("nifty-link", "Interface rename"),
        ("nifty-ro", "Root remount (ro)"),
    ];
    for (service, label) in services {
        let output = Command::new("systemctl")
            .args(["is-active", service])
            .output();
        let state = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "unknown".to_string(),
        };
        let icon = match state.as_str() {
            "active" => "ok",
            "inactive" => "--",
            _ => "FAIL",
        };
        println!("  [{icon:^4}] {label:<20} ({service})");
    }
    println!();
    println!("  === Filesystem Status ===");
    for mount in ["/", "/nix/store", "/var"] {
        let output = Command::new("findmnt")
            .args(["-n", "-o", "OPTIONS", mount])
            .output();
        let opts = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => String::new(),
        };
        let state = if opts.split(',').any(|o| o == "ro") { "ro" } else { "rw" };
        let df = Command::new("df")
            .args(["-h", "--output=used,size", mount])
            .output();
        let usage = match df {
            Ok(o) => {
                let out = String::from_utf8_lossy(&o.stdout).to_string();
                out.lines().nth(1).unwrap_or("").split_whitespace()
                    .collect::<Vec<_>>().join(" / ")
            }
            Err(_) => String::new(),
        };
        println!("  [{state:^4}] {mount:<12} {usage}");
    }
    println!();
}

fn menu_logs() {
    let services = [
        ("nifty-filter", "Firewall"),
        ("nifty-network", "Network"),
        ("nifty-dnsmasq", "DHCP / DNS"),
        ("nifty-hostname", "Hostname"),
        ("nifty-link", "Interface rename"),
        ("nifty-ro", "Root remount (ro)"),
    ];
    let mut cursor = 0;
    loop {
        let mut items: Vec<String> = services
            .iter()
            .flat_map(|(_, label)| [
                label.to_string(),
                format!("{label} (live)"),
            ])
            .collect();
        items.push("All (this boot)".to_string());
        items.push("All (live)".to_string());
        items.push("Back".to_string());

        match choose("Show logs:", items, cursor) {
            Some((idx, choice)) if choice == "Back" => break,
            Some((idx, choice)) if choice == "All (this boot)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-b"]));
            }
            Some((idx, choice)) if choice == "All (live)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-f"]));
            }
            Some((idx, choice)) => {
                cursor = idx;
                let service_idx = idx / 2;
                if let Some((unit, _)) = services.get(service_idx) {
                    if choice.ends_with("(live)") {
                        run_interactive(Command::new("journalctl").args(["-u", unit, "-f"]));
                    } else {
                        run_interactive(Command::new("journalctl").args(["-u", unit, "-b"]));
                    }
                }
            }
            None => break,
        }
    }
}

fn show_config(env: &EnvFile) {
    println!();
    println!("  === Current Configuration ===");
    println!("  ENABLED:        {}", env.get("ENABLED"));
    println!("  HOSTNAME:       {}", env.get("HOSTNAME"));
    println!("  INTERFACE_WAN:  {}", env.get("INTERFACE_WAN"));
    println!("  INTERFACE_LAN:  {}", env.get("INTERFACE_LAN"));
    println!("  ENABLE_IPV4:    {}", if env.get("ENABLE_IPV4").is_empty() { "true" } else { env.get("ENABLE_IPV4") });
    println!("  ENABLE_IPV6:    {}", if env.get("ENABLE_IPV6").is_empty() { "false" } else { env.get("ENABLE_IPV6") });
    println!("  SUBNET_LAN:     {}", env.get("SUBNET_LAN"));
    let ipv6_subnet = env.get("SUBNET_LAN_IPV6");
    if !ipv6_subnet.is_empty() {
        println!("  SUBNET_LAN_IPV6: {ipv6_subnet}");
    }
    println!("  TCP_ACCEPT_LAN: {}", env.get("TCP_ACCEPT_LAN"));
    println!("  UDP_ACCEPT_LAN: {}", env.get("UDP_ACCEPT_LAN"));
    println!("  TCP_ACCEPT_WAN: {}", env.get("TCP_ACCEPT_WAN"));
    println!("  UDP_ACCEPT_WAN: {}", env.get("UDP_ACCEPT_WAN"));
    let egress4 = env.get("LAN_EGRESS_ALLOWED_IPV4");
    if !egress4.is_empty() {
        println!("  EGRESS_IPV4:    {egress4}");
    }
    let egress6 = env.get("LAN_EGRESS_ALLOWED_IPV6");
    if !egress6.is_empty() {
        println!("  EGRESS_IPV6:    {egress6}");
    }
    println!("  TCP_FORWARD_LAN: {}", env.get("TCP_FORWARD_LAN"));
    println!("  UDP_FORWARD_LAN: {}", env.get("UDP_FORWARD_LAN"));
    println!("  TCP_FORWARD_WAN: {}", env.get("TCP_FORWARD_WAN"));
    println!("  UDP_FORWARD_WAN: {}", env.get("UDP_FORWARD_WAN"));
    println!(
        "  DHCP_POOL:      {} - {}",
        env.get("DHCP_POOL_START"),
        env.get("DHCP_POOL_END")
    );
    println!("  DHCP_DNS:       {}", env.get("DHCP_DNS"));
    if env.get("DHCPV6_ENABLED") == "true" {
        println!(
            "  DHCPV6:         {} - {}",
            env.get("DHCPV6_POOL_START"),
            env.get("DHCPV6_POOL_END")
        );
    }
    println!();
}

fn list_interfaces() -> Vec<String> {
    let output = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    output
        .lines()
        .filter_map(|line| {
            let name = line.split(':').nth(1)?.trim().to_string();
            if name == "lo" { None } else { Some(name) }
        })
        .collect()
}

fn get_mac(iface: &str) -> String {
    let output = Command::new("ip")
        .args(["-o", "link", "show", iface])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    output
        .split("link/ether ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or("")
        .to_string()
}

fn prompt_validated<F>(message: &str, default: &str, validate: F) -> Option<String>
where
    F: Fn(&str) -> Option<&'static str>,
{
    loop {
        let v = prompt_text(message, default)?;
        if let Some(err) = validate(&v) {
            println!("  {err}");
        } else {
            return Some(v);
        }
    }
}

fn reset_config() -> Option<EnvFile> {
    println!();
    println!("  This will erase your current configuration and start fresh.");
    println!();
    loop {
        match Text::new("Type 'reset' to confirm, or 'cancel':").prompt() {
            Ok(v) if v.trim().eq_ignore_ascii_case("reset") => break,
            Ok(v) if v.trim().eq_ignore_ascii_case("cancel") => return None,
            Ok(_) => continue,
            Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => return None,
            Err(_) => return None,
        }
    }

    let hostname_re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();

    // Hostname
    println!();
    println!("==> Configure hostname:");
    let hostname = prompt_validated("Hostname for this router", "nifty-filter", |v| {
        if hostname_re.is_match(v) { None }
        else { Some("Invalid hostname. Must be 1-63 chars: letters, digits, hyphens.") }
    })?;

    // Interfaces
    println!();
    println!("==> Configure network interfaces:");
    let ifaces = list_interfaces();
    if ifaces.len() < 2 {
        println!("  Need at least 2 network interfaces (found {}).", ifaces.len());
        return None;
    }

    let wan_choice = choose("Select WAN interface (upstream/internet):", ifaces.clone(), 0)?;
    let wan_iface = wan_choice.1;
    println!("  WAN: {wan_iface} -> wan");

    let lan_ifaces: Vec<String> = ifaces.into_iter().filter(|i| i != &wan_iface).collect();
    let lan_iface = if lan_ifaces.len() == 1 {
        println!("  LAN: {} -> lan (only remaining interface)", lan_ifaces[0]);
        lan_ifaces[0].clone()
    } else {
        let choice = choose("Select LAN interface (local network):", lan_ifaces, 0)?;
        println!("  LAN: {} -> lan", choice.1);
        choice.1
    };

    let wan_mac = get_mac(&wan_iface);
    let lan_mac = get_mac(&lan_iface);

    // Subnet
    println!();
    println!("==> Configure LAN network:");
    let subnet_lan = prompt_validated("LAN subnet (IP/prefix)", "10.99.0.1/24", |v| {
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() { None }
        else { Some("Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).") }
    })?;

    let router_ip = subnet_lan.split_once('/').map(|(ip, _)| ip).unwrap_or(&subnet_lan).to_string();
    let network_base = router_ip.rsplit_once('.').map(|(base, _)| base).unwrap_or(&router_ip).to_string();
    let default_start = format!("{network_base}.100");
    let default_end = format!("{network_base}.250");

    // DHCP
    println!();
    println!("==> Configure DHCP pool:");
    let dhcp_start = prompt_text("DHCP pool start", &default_start)?;
    let dhcp_end = prompt_text("DHCP pool end", &default_end)?;
    let dns_servers = prompt_text("DNS servers (comma-separated)", "1.1.1.1, 1.0.0.1")?;

    // Write env file
    let env_content = format!(
        r#"# nifty-filter configuration
# Edit this file and run: nifty-config -> Apply changes
#
# This file lives on the writable /var partition.
# The rest of the system is read-only (unless booted in maintenance mode).
ENABLED=true
HOSTNAME={hostname}

# Network interfaces
INTERFACE_LAN=lan
INTERFACE_WAN=wan

# LAN subnet in CIDR notation (router's LAN IP / prefix length)
SUBNET_LAN={subnet}

# ICMP types accepted on each interface
ICMP_ACCEPT_LAN=echo-request,echo-reply,destination-unreachable,time-exceeded
ICMP_ACCEPT_WAN=

# TCP/UDP ports the router itself accepts
TCP_ACCEPT_LAN=22
UDP_ACCEPT_LAN=67,68
TCP_ACCEPT_WAN=22
UDP_ACCEPT_WAN=

# Port forwarding rules
# Format: incoming_port:destination_ip:destination_port
TCP_FORWARD_LAN=
UDP_FORWARD_LAN=
TCP_FORWARD_WAN=
UDP_FORWARD_WAN=

# DHCP server configuration
DHCP_INTERFACE=lan
DHCP_SUBNET={subnet}
DHCP_POOL_START={dhcp_start}
DHCP_POOL_END={dhcp_end}
DHCP_ROUTER={router_ip}
DHCP_DNS={dns}

# DHCPv6 (enable and configure via nifty-config after install)
DHCPV6_ENABLED=false
DHCPV6_POOL_START=
DHCPV6_POOL_END=
"#,
        hostname = hostname,
        subnet = subnet_lan,
        dhcp_start = dhcp_start,
        dhcp_end = dhcp_end,
        router_ip = router_ip,
        dns = dns_servers,
    );

    // Write interface rename rules
    let network_dir = "/var/nifty-filter/network";
    fs::create_dir_all(network_dir).ok();
    fs::write(
        format!("{network_dir}/10-wan.link"),
        format!("[Match]\nMACAddress={wan_mac}\n\n[Link]\nName=wan\n"),
    ).ok();
    fs::write(
        format!("{network_dir}/10-lan.link"),
        format!("[Match]\nMACAddress={lan_mac}\n\n[Link]\nName=lan\n"),
    ).ok();

    fs::write(ENV_FILE, &env_content).ok();
    let _ = Command::new("chmod").args(["0600", ENV_FILE]).status();
    let _ = Command::new("sudo").args(["rm", "-f", "/var/lib/dnsmasq/dnsmasq.leases"]).status();

    println!();
    println!("  Configuration reset. Apply changes or reboot to activate.");

    // Reload
    match EnvFile::load(std::path::Path::new(ENV_FILE)) {
        Ok(env) => Some(env),
        Err(e) => {
            eprintln!("  Warning: could not reload config: {e}");
            None
        }
    }
}

fn launch_editor(path: &str) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    let _ = Command::new(&editor).arg(path).status();
}

// --- Submenus ---

fn menu_network(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = env.get("ENABLE_IPV6") == "true";
        let ipv6_label = if ipv6_enabled {
            "Disable IPv6"
        } else {
            "Enable IPv6"
        };

        let mut items = vec![
            format!("Hostname ({})", env.get("HOSTNAME")),
            format!("LAN IPv4 subnet ({})", env.get("SUBNET_LAN")),
        ];
        if ipv6_enabled {
            items.push(format!(
                "LAN IPv6 subnet ({})",
                env.get("SUBNET_LAN_IPV6")
            ));
        }
        items.push(ipv6_label.to_string());
        items.push("Back".to_string());

        match choose("Network:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("Hostname") => { cursor = idx; edit_hostname(env) }
            Some((idx, choice)) if choice.starts_with("LAN IPv4 subnet") => { cursor = idx; edit_subnet(env) }
            Some((idx, choice)) if choice.starts_with("LAN IPv6 subnet") => { cursor = idx; edit_subnet_ipv6(env) }
            Some((idx, choice)) if choice == "Enable IPv6" || choice == "Disable IPv6" => {
                cursor = idx; toggle_ipv6(env)
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_firewall(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = env.get("ENABLE_IPV6") == "true";

        let mut items = vec![
            format!("TCP ports LAN ({})", env.get("TCP_ACCEPT_LAN")),
            format!("UDP ports LAN ({})", env.get("UDP_ACCEPT_LAN")),
            format!("TCP ports WAN ({})", env.get("TCP_ACCEPT_WAN")),
            format!("UDP ports WAN ({})", env.get("UDP_ACCEPT_WAN")),
            format!(
                "Egress filter IPv4 ({})",
                env.get("LAN_EGRESS_ALLOWED_IPV4")
            ),
        ];
        if ipv6_enabled {
            items.push(format!(
                "Egress filter IPv6 ({})",
                env.get("LAN_EGRESS_ALLOWED_IPV6")
            ));
        }
        items.push("Back".to_string());

        match choose("Firewall:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("TCP ports LAN") => {
                cursor = idx; edit_ports(env, "TCP_ACCEPT_LAN", "TCP ports LAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP ports LAN") => {
                cursor = idx; edit_ports(env, "UDP_ACCEPT_LAN", "UDP ports LAN")
            }
            Some((idx, choice)) if choice.starts_with("TCP ports WAN") => {
                cursor = idx; edit_ports(env, "TCP_ACCEPT_WAN", "TCP ports WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP ports WAN") => {
                cursor = idx; edit_ports(env, "UDP_ACCEPT_WAN", "UDP ports WAN")
            }
            Some((idx, choice)) if choice.starts_with("Egress filter IPv4") => { cursor = idx; edit_egress_ipv4(env) }
            Some((idx, choice)) if choice.starts_with("Egress filter IPv6") => { cursor = idx; edit_egress_ipv6(env) }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_port_forwarding(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let items = vec![
            format!("TCP forward LAN ({})", env.get("TCP_FORWARD_LAN")),
            format!("UDP forward LAN ({})", env.get("UDP_FORWARD_LAN")),
            format!("TCP forward WAN ({})", env.get("TCP_FORWARD_WAN")),
            format!("UDP forward WAN ({})", env.get("UDP_FORWARD_WAN")),
            "Back".to_string(),
        ];

        match choose("Port Forwarding:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("TCP forward LAN") => {
                cursor = idx; edit_forwards(env, "TCP_FORWARD_LAN", "TCP forward LAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP forward LAN") => {
                cursor = idx; edit_forwards(env, "UDP_FORWARD_LAN", "UDP forward LAN")
            }
            Some((idx, choice)) if choice.starts_with("TCP forward WAN") => {
                cursor = idx; edit_forwards(env, "TCP_FORWARD_WAN", "TCP forward WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP forward WAN") => {
                cursor = idx; edit_forwards(env, "UDP_FORWARD_WAN", "UDP forward WAN")
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_dhcp_dns(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let ipv6_enabled = env.get("ENABLE_IPV6") == "true";
        let dhcpv6_enabled = env.get("DHCPV6_ENABLED") == "true";
        let dhcp4_val = env.get("DHCP4_ENABLED");
        let dhcp4_enabled = dhcp4_val.is_empty() || dhcp4_val == "true";

        let dhcp4_label = if dhcp4_enabled {
            "Disable DHCPv4"
        } else {
            "Enable DHCPv4"
        };

        let mut items = vec![
            dhcp4_label.to_string(),
        ];
        if dhcp4_enabled {
            items.push(format!(
                "DHCP pool ({} - {})",
                env.get("DHCP_POOL_START"),
                env.get("DHCP_POOL_END")
            ));
        }
        items.push(format!("DNS servers ({})", env.get("DHCP_DNS")));
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
                    env.get("DHCPV6_POOL_START"),
                    env.get("DHCPV6_POOL_END")
                ));
            }
        }
        items.push("Back".to_string());

        match choose("DHCP / DNS:", items, cursor) {
            Some((idx, choice)) if choice == "Enable DHCPv4" || choice == "Disable DHCPv4" => {
                cursor = idx; toggle_dhcp4(env)
            }
            Some((idx, choice)) if choice.starts_with("DHCP pool") => { cursor = idx; edit_dhcp_pool(env) }
            Some((idx, choice)) if choice.starts_with("DNS servers") => { cursor = idx; edit_dns(env) }
            Some((idx, choice)) if choice == "Enable DHCPv6" || choice == "Disable DHCPv6" => {
                cursor = idx; toggle_dhcpv6(env)
            }
            Some((idx, choice)) if choice.starts_with("DHCPv6 pool") => { cursor = idx; edit_dhcpv6_pool(env) }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

// --- Main menu ---

pub fn run() {
    let mut env = match EnvFile::load(std::path::Path::new(ENV_FILE)) {
        Ok(env) => env,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };

    let mut cursor = 0;
    loop {
        println!();
        let enabled_label = if env.get("ENABLED") == "true" {
            "Disable firewall"
        } else {
            "Enable firewall"
        };

        let items = vec![
            "Show config".to_string(),
            "Show status".to_string(),
            "Show logs".to_string(),
            "Network".to_string(),
            "Firewall".to_string(),
            "Port forwarding".to_string(),
            "DHCP / DNS".to_string(),
            enabled_label.to_string(),
            "Apply changes".to_string(),
            "Edit nifty-filter.env".to_string(),
            "Reset config".to_string(),
            "Reboot".to_string(),
            "Quit".to_string(),
        ];

        match choose("nifty-filter configuration:", items, cursor) {
            Some((idx, choice)) => { cursor = idx; match choice.as_str() {
                "Show config" => show_config(&env),
                "Show status" => show_status(),
                "Show logs" => menu_logs(),
                "Network" => menu_network(&mut env),
                "Firewall" => menu_firewall(&mut env),
                "Port forwarding" => menu_port_forwarding(&mut env),
                "DHCP / DNS" => menu_dhcp_dns(&mut env),
                "Enable firewall" | "Disable firewall" => toggle_enabled(&mut env),
                "Apply changes" => apply_changes(&env),
                "Edit nifty-filter.env" => {
                    launch_editor(ENV_FILE);
                    if let Err(e) = env.reload() {
                        eprintln!("  Warning: {e}");
                    }
                }
                "Reset config" => {
                    if let Some(new_env) = reset_config() {
                        env = new_env;
                    }
                }
                "Reboot" => {
                    if let Some(v) = prompt_text("Reboot now? (yes/no)", "no") {
                        if v.trim().eq_ignore_ascii_case("yes") || v.trim().eq_ignore_ascii_case("y") {
                            let _ = Command::new("sudo").args(["systemctl", "reboot"]).status();
                        }
                    }
                }
                "Quit" => break,
                _ => {}
            }},
            None => break, // ESC at main menu = quit
        }
    }
}
