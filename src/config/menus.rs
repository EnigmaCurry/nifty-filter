use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use inquire::{InquireError, Select, Text};
use ipnetwork::IpNetwork;
use regex::Regex;

use crate::hcl_config::*;
use super::hcl_file;

const HCL_FILE: &str = "/var/nifty-filter/nifty-filter.hcl";

/// Run a command in its own process group so Ctrl-C only kills the child.
fn run_interactive(cmd: &mut Command) {
    // Ignore SIGINT and SIGTTOU in parent while child runs
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
    }
    unsafe {
        cmd.pre_exec(|| {
            // New process group for the child
            libc::setpgid(0, 0);
            // Make child the foreground process group
            libc::tcsetpgrp(0, libc::getpid());
            Ok(())
        });
    }
    if let Ok(mut child) = cmd.spawn() {
        let _ = child.wait();
    }
    // Restore parent as foreground process group
    unsafe {
        libc::tcsetpgrp(0, libc::getpgrp());
        libc::signal(libc::SIGTTOU, libc::SIG_DFL);
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

fn pool_size_ipv4(start: &str, end: &str) -> Option<u64> {
    let s: std::net::Ipv4Addr = start.parse().ok()?;
    let e: std::net::Ipv4Addr = end.parse().ok()?;
    let s = u32::from(s);
    let e = u32::from(e);
    if e >= s {
        Some((e - s + 1) as u64)
    } else {
        None
    }
}

fn pool_size_ipv6(start: &str, end: &str) -> Option<u128> {
    let s: std::net::Ipv6Addr = start.parse().ok()?;
    let e: std::net::Ipv6Addr = end.parse().ok()?;
    let s = u128::from(s);
    let e = u128::from(e);
    if e >= s {
        Some(e - s + 1)
    } else {
        None
    }
}

fn pool_label_v4(start: &str, end: &str) -> String {
    match pool_size_ipv4(start, end) {
        Some(n) => format!("{start} - {end} ({n} addrs)"),
        None => format!("{start} - {end}"),
    }
}

fn pool_label_v6(start: &str, end: &str) -> String {
    match pool_size_ipv6(start, end) {
        Some(n) => format!("{start} - {end} ({n} addrs)"),
        None => format!("{start} - {end}"),
    }
}

fn format_count(n: u128) -> String {
    if n < 10_000 {
        format!("{n}")
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else if n < 1_000_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n < 1_000_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n < 1_000_000_000_000_000 {
        format!("{:.1}T", n as f64 / 1_000_000_000_000.0)
    } else {
        format!(
            "{:.1}e{}",
            n as f64 / 10f64.powi(n.ilog10() as i32),
            n.ilog10()
        )
    }
}

fn subnet_label(cidr: &str) -> String {
    if cidr.is_empty() {
        return String::new();
    }
    match cidr.parse::<IpNetwork>() {
        Ok(net) => {
            let prefix = net.prefix();
            let size: u128 = match net {
                IpNetwork::V4(_) => 1u128 << (32 - prefix),
                IpNetwork::V6(_) => 1u128 << (128 - prefix),
            };
            format!("{cidr} ({} addrs)", format_count(size))
        }
        Err(_) => cidr.to_string(),
    }
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

// --- VLAN helpers ---

/// Return VLANs sorted by id, as (name, config) pairs.
fn sorted_vlans(config: &HclConfig) -> Vec<(String, &VlanHclConfig)> {
    let mut vlans: Vec<_> = config
        .vlan
        .iter()
        .map(|(k, v)| (k.clone(), v))
        .collect();
    vlans.sort_by_key(|(_, v)| v.id);
    vlans
}

fn save_config(config: &HclConfig) {
    if let Err(e) = hcl_file::save(config, Path::new(HCL_FILE)) {
        eprintln!("  Error saving config: {e}");
    }
}

// --- Editor functions ---

fn edit_hostname(config: &mut HclConfig) {
    let current = config.hostname.as_deref().unwrap_or("");
    let val = match prompt_text("Hostname", current) {
        Some(v) => v,
        None => return,
    };
    let re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();
    if re.is_match(&val) {
        config.hostname = Some(val.clone());
        save_config(config);
        println!("  Set hostname = \"{val}\"");
    } else {
        println!("  Invalid hostname.");
    }
}

fn edit_vlan_subnet(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let current = vlan
        .ipv4
        .as_ref()
        .map(|v| v.subnet.as_str())
        .unwrap_or("");
    let default = if current.is_empty() {
        "10.99.1.1/24"
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(
            &format!("VLAN \"{vlan_name}\" IPv4 subnet (IP/prefix)"),
            default,
        ) {
            Some(v) => v,
            None => return,
        };
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).");
    };

    let vlan = config.vlan.get_mut(vlan_name).unwrap();

    // Extract router IP and network base for DHCP defaults
    let router_ip = val
        .split_once('/')
        .map(|(ip, _)| ip)
        .unwrap_or(&val)
        .to_string();
    let network_base = router_ip
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(&router_ip)
        .to_string();

    // Update or create ipv4 block
    if let Some(ref mut ipv4) = vlan.ipv4 {
        ipv4.subnet = val.clone();
    } else {
        vlan.ipv4 = Some(Ipv4Config {
            subnet: val.clone(),
            egress: vec!["0.0.0.0/0".to_string()],
        });
    }

    // Update DHCP defaults to match
    if let Some(ref mut dhcp) = vlan.dhcp {
        dhcp.router = router_ip.clone();
        dhcp.dns = router_ip;
        dhcp.pool_start = format!("{network_base}.100");
        dhcp.pool_end = format!("{network_base}.250");
        println!("  Updated DHCP pool to match.");
    }

    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" IPv4 subnet = \"{val}\"");
}

fn edit_vlan_subnet_ipv6(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let current = vlan
        .ipv6
        .as_ref()
        .map(|v| v.subnet.as_str())
        .unwrap_or("");
    let default = if current.is_empty() {
        "fd00:10::1/64"
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(
            &format!("VLAN \"{vlan_name}\" IPv6 subnet (IP/prefix)"),
            default,
        ) {
            Some(v) => v,
            None => return,
        };
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. fd00:10::1/64).");
    };

    let vlan = config.vlan.get_mut(vlan_name).unwrap();

    // Update or create ipv6 block
    if let Some(ref mut ipv6) = vlan.ipv6 {
        ipv6.subnet = val.clone();
    } else {
        vlan.ipv6 = Some(Ipv6Config {
            subnet: val.clone(),
            egress: vec!["::/0".to_string()],
        });
    }

    // Update DHCPv6 pool to match if enabled
    if let Some(ref mut dhcpv6) = vlan.dhcpv6 {
        if let Some((addr, _)) = val.split_once('/') {
            if let Some((prefix, _)) = addr.rsplit_once(':') {
                dhcpv6.pool_start = format!("{prefix}:100");
                dhcpv6.pool_end = format!("{prefix}:1ff");
                println!("  Updated DHCPv6 pool to match.");
            }
        }
    }

    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" IPv6 subnet = \"{val}\"");
}

fn toggle_ipv6(config: &mut HclConfig) {
    config.wan.enable_ipv6 = !config.wan.enable_ipv6;
    save_config(config);
    if config.wan.enable_ipv6 {
        println!("  IPv6 enabled. Configure per-VLAN IPv6 subnets in the Network menu.");
    } else {
        println!("  IPv6 disabled.");
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

fn parse_cidr_list(input: &str) -> Vec<String> {
    if input.is_empty() {
        return vec![];
    }
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn format_cidr_list(cidrs: &[String]) -> String {
    cidrs.join(", ")
}

fn edit_vlan_egress_ipv4(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let current = vlan
        .ipv4
        .as_ref()
        .map(|v| format_cidr_list(&v.egress))
        .unwrap_or_default();
    let default = if current.is_empty() {
        "0.0.0.0/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(
            &format!("VLAN \"{vlan_name}\" allowed IPv4 egress CIDRs"),
            &default,
        ) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like 0.0.0.0/0 or 10.0.0.0/8,172.16.0.0/12.");
    };
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if let Some(ref mut ipv4) = vlan.ipv4 {
        ipv4.egress = parse_cidr_list(&val);
    }
    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" egress IPv4 = [{val}]");
}

fn edit_vlan_egress_ipv6(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let current = vlan
        .ipv6
        .as_ref()
        .map(|v| format_cidr_list(&v.egress))
        .unwrap_or_default();
    let default = if current.is_empty() {
        "::/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(
            &format!("VLAN \"{vlan_name}\" allowed IPv6 egress CIDRs"),
            &default,
        ) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like ::/0 or fd00::/8.");
    };
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if let Some(ref mut ipv6) = vlan.ipv6 {
        ipv6.egress = parse_cidr_list(&val);
    }
    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" egress IPv6 = [{val}]");
}

fn format_ports(ports: &[u16]) -> String {
    ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_ports(input: &str) -> Vec<u16> {
    if input.is_empty() {
        return vec![];
    }
    input
        .split(',')
        .filter_map(|s| s.trim().parse::<u16>().ok())
        .collect()
}

fn edit_ports(ports: &mut Vec<u16>, label: &str) {
    let current = format_ports(ports);
    let val = match prompt_text_allow_blank(&format!("{label} (comma-separated)"), &current) {
        Some(v) => v,
        None => return,
    };
    *ports = parse_ports(&val);
    println!("  Set {label} = [{}]", format_ports(ports));
}

fn format_forwards(forwards: &[String]) -> String {
    forwards.join(", ")
}

fn parse_forwards(input: &str) -> Vec<String> {
    if input.is_empty() {
        return vec![];
    }
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn edit_forwards(forwards: &mut Vec<String>, label: &str) {
    let current = format_forwards(forwards);
    println!("  Format: incoming_port:dest_ip:dest_port (comma-separated)");
    println!("  IPv6:   incoming_port:[ipv6_addr]:dest_port");
    let val = match prompt_text_allow_blank(label, &current) {
        Some(v) => v,
        None => return,
    };
    *forwards = parse_forwards(&val);
    println!("  Set {label}");
}

fn edit_vlan_dhcp_pool(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let (start, end) = match &vlan.dhcp {
        Some(d) => (d.pool_start.clone(), d.pool_end.clone()),
        None => return,
    };
    let start = match prompt_text(
        &format!("VLAN \"{vlan_name}\" DHCP pool start"),
        &start,
    ) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text(
        &format!("VLAN \"{vlan_name}\" DHCP pool end"),
        &end,
    ) {
        Some(v) => v,
        None => return,
    };
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if let Some(ref mut dhcp) = vlan.dhcp {
        dhcp.pool_start = start.clone();
        dhcp.pool_end = end.clone();
    }
    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" DHCP pool: {start} - {end}");
}

fn edit_vlan_dns(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let current = vlan
        .dhcp
        .as_ref()
        .map(|d| d.dns.as_str())
        .unwrap_or("");
    let val = match prompt_text(
        &format!("VLAN \"{vlan_name}\" DNS servers"),
        current,
    ) {
        Some(v) => v,
        None => return,
    };
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if let Some(ref mut dhcp) = vlan.dhcp {
        dhcp.dns = val.clone();
    }
    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" DNS = \"{val}\"");
}

fn toggle_vlan_dhcp4(config: &mut HclConfig, vlan_name: &str) {
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if vlan.dhcp.is_some() {
        // Disable: remove the dhcp block
        vlan.dhcp = None;
        // Remove DHCP ports from firewall
        if let Some(ref mut fw) = vlan.firewall {
            fw.udp_accept.retain(|p| *p != 67 && *p != 68);
        }
        save_config(config);
        println!("  VLAN \"{vlan_name}\" DHCPv4 disabled.");
    } else {
        // Enable: create dhcp block with defaults from subnet
        let router_ip = vlan
            .ipv4
            .as_ref()
            .map(|v| {
                v.subnet
                    .split_once('/')
                    .map(|(ip, _)| ip)
                    .unwrap_or(&v.subnet)
                    .to_string()
            })
            .unwrap_or_else(|| "10.99.1.1".to_string());
        let network_base = router_ip
            .rsplit_once('.')
            .map(|(base, _)| base)
            .unwrap_or(&router_ip)
            .to_string();
        vlan.dhcp = Some(DhcpConfig {
            pool_start: format!("{network_base}.100"),
            pool_end: format!("{network_base}.250"),
            router: router_ip.clone(),
            dns: router_ip,
            host: vec![],
        });
        // Add DHCP ports to firewall
        if let Some(ref mut fw) = vlan.firewall {
            if !fw.udp_accept.contains(&67) {
                fw.udp_accept.push(67);
            }
            if !fw.udp_accept.contains(&68) {
                fw.udp_accept.push(68);
            }
        }
        save_config(config);
        println!("  VLAN \"{vlan_name}\" DHCPv4 enabled.");
    }
}

fn toggle_vlan_dhcpv6(config: &mut HclConfig, vlan_name: &str) {
    let has_dhcpv6 = config
        .vlan
        .get(vlan_name)
        .map(|v| v.dhcpv6.is_some())
        .unwrap_or(false);

    if has_dhcpv6 {
        // Disable
        let vlan = config.vlan.get_mut(vlan_name).unwrap();
        vlan.dhcpv6 = None;
        if let Some(ref mut fw) = vlan.firewall {
            fw.udp_accept.retain(|p| *p != 546 && *p != 547);
        }
        save_config(config);
        println!("  VLAN \"{vlan_name}\" DHCPv6 disabled.");
    } else {
        // Enable: derive pool from IPv6 subnet
        let needs_pool_edit;
        {
            let vlan = config.vlan.get_mut(vlan_name).unwrap();
            let (pool_start, pool_end) = vlan
                .ipv6
                .as_ref()
                .and_then(|v6| {
                    let (addr, _) = v6.subnet.split_once('/')?;
                    let (prefix, _) = addr.rsplit_once(':')?;
                    Some((format!("{prefix}:100"), format!("{prefix}:1ff")))
                })
                .unwrap_or_else(|| ("::100".to_string(), "::1ff".to_string()));

            needs_pool_edit = vlan
                .ipv6
                .as_ref()
                .map(|v| v.subnet.is_empty())
                .unwrap_or(true);

            vlan.dhcpv6 = Some(Dhcpv6Config {
                pool_start,
                pool_end,
            });
            // Add DHCPv6 ports to firewall
            if let Some(ref mut fw) = vlan.firewall {
                if !fw.udp_accept.contains(&546) {
                    fw.udp_accept.push(546);
                }
                if !fw.udp_accept.contains(&547) {
                    fw.udp_accept.push(547);
                }
            }
        }
        save_config(config);

        if needs_pool_edit {
            println!("  DHCPv6 requires a pool range.");
            edit_vlan_dhcpv6_pool(config, vlan_name);
        }
        println!("  VLAN \"{vlan_name}\" DHCPv6 enabled.");
    }
}

fn edit_vlan_dhcpv6_pool(config: &mut HclConfig, vlan_name: &str) {
    let vlan = match config.vlan.get(vlan_name) {
        Some(v) => v,
        None => return,
    };
    let (mut start, mut end) = match &vlan.dhcpv6 {
        Some(d) => (d.pool_start.clone(), d.pool_end.clone()),
        None => return,
    };
    if start.is_empty() {
        if let Some(ref ipv6) = vlan.ipv6 {
            if let Some((addr, _)) = ipv6.subnet.split_once('/') {
                if let Some((prefix, _)) = addr.rsplit_once(':') {
                    start = format!("{prefix}:100");
                    end = format!("{prefix}:1ff");
                }
            }
        }
    }
    let start = match prompt_text(
        &format!("VLAN \"{vlan_name}\" DHCPv6 pool start"),
        &start,
    ) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text(
        &format!("VLAN \"{vlan_name}\" DHCPv6 pool end"),
        &end,
    ) {
        Some(v) => v,
        None => return,
    };
    let vlan = config.vlan.get_mut(vlan_name).unwrap();
    if let Some(ref mut dhcpv6) = vlan.dhcpv6 {
        dhcpv6.pool_start = start.clone();
        dhcpv6.pool_end = end.clone();
    }
    save_config(config);
    println!("  Set VLAN \"{vlan_name}\" DHCPv6 pool: {start} - {end}");
}

fn apply_changes(config: &HclConfig) {
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
    if let Some(ref hostname) = config.hostname {
        println!("  Setting hostname...");
        let _ = Command::new("sudo")
            .args(["hostname", hostname])
            .status();
    }
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
        let state = if opts.split(',').any(|o| o == "ro") {
            "ro"
        } else {
            "rw"
        };
        let df = Command::new("df")
            .args(["-h", "--output=used,size", mount])
            .output();
        let usage = match df {
            Ok(o) => {
                let out = String::from_utf8_lossy(&o.stdout).to_string();
                out.lines()
                    .nth(1)
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
            Err(_) => String::new(),
        };
        println!("  [{state:^4}] {mount:<12} {usage}");
    }
    println!();
}

fn menu_logs() {
    let services: &[(&str, &str, bool)] = &[
        ("nifty-filter", "Firewall", false),
        ("nifty-dnsmasq", "DHCP / DNS", true),
        ("nifty-network", "Network", false),
        ("nifty-hostname", "Hostname", false),
        ("nifty-link", "Interface rename", false),
        ("nifty-ro", "Root remount (ro)", false),
    ];
    let mut cursor = 0;
    loop {
        let mut items: Vec<String> = Vec::new();
        let mut item_actions: Vec<(&str, bool)> = Vec::new();
        for &(unit, label, has_live) in services {
            items.push(label.to_string());
            item_actions.push((unit, false));
            if has_live {
                items.push(format!("{label} (live)"));
                item_actions.push((unit, true));
            }
        }
        items.push("All (this boot)".to_string());
        items.push("All (last boot)".to_string());
        items.push("All (live)".to_string());
        items.push("Back".to_string());

        match choose("Show logs:", items, cursor) {
            Some((_, ref choice)) if choice == "Back" => break,
            Some((idx, ref choice)) if choice == "All (this boot)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-b"]));
                println!();
            }
            Some((idx, ref choice)) if choice == "All (last boot)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-b", "-1"]));
                println!();
            }
            Some((idx, ref choice)) if choice == "All (live)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-f"]));
                println!();
            }
            Some((idx, _)) => {
                cursor = idx;
                if let Some(&(unit, live)) = item_actions.get(idx) {
                    if live {
                        run_interactive(Command::new("journalctl").args(["-u", unit, "-f"]));
                    } else {
                        run_interactive(Command::new("journalctl").args(["-u", unit, "-b"]));
                    }
                    println!();
                }
            }
            None => break,
        }
    }
}

fn show_config(config: &HclConfig) {
    println!();
    println!("  === Current Configuration ===");
    println!(
        "  hostname:       {}",
        config.hostname.as_deref().unwrap_or("(not set)")
    );
    println!(
        "  trunk:          {} ({})",
        config.interfaces.trunk.name,
        config
            .interfaces
            .trunk
            .mac
            .as_deref()
            .unwrap_or("no MAC")
    );
    println!(
        "  wan:            {} ({})",
        config.interfaces.wan.name,
        config.interfaces.wan.mac.as_deref().unwrap_or("no MAC")
    );
    if let Some(ref mgmt) = config.interfaces.mgmt {
        println!(
            "  mgmt:           {} ({}) {}",
            mgmt.name,
            mgmt.mac.as_deref().unwrap_or("no MAC"),
            mgmt.subnet.as_deref().unwrap_or("")
        );
    }
    println!("  enable_ipv4:    {}", config.wan.enable_ipv4);
    println!("  enable_ipv6:    {}", config.wan.enable_ipv6);
    println!(
        "  switch mode:    {}",
        if config.vlan_aware_switch {
            "VLAN-aware"
        } else {
            "simple"
        }
    );

    for (name, vlan) in sorted_vlans(config) {
        println!();
        println!("  --- VLAN \"{}\" (id={}) ---", name, vlan.id);
        if let Some(ref ipv4) = vlan.ipv4 {
            println!("  IPv4 subnet:    {}", subnet_label(&ipv4.subnet));
            println!(
                "  egress IPv4:    {}",
                if ipv4.egress.is_empty() {
                    "deny".to_string()
                } else {
                    format_cidr_list(&ipv4.egress)
                }
            );
        }
        if let Some(ref ipv6) = vlan.ipv6 {
            println!("  IPv6 subnet:    {}", subnet_label(&ipv6.subnet));
            println!(
                "  egress IPv6:    {}",
                if ipv6.egress.is_empty() {
                    "deny".to_string()
                } else {
                    format_cidr_list(&ipv6.egress)
                }
            );
        }
        if let Some(ref fw) = vlan.firewall {
            println!("  TCP accept:     {}", format_ports(&fw.tcp_accept));
            println!("  UDP accept:     {}", format_ports(&fw.udp_accept));
        }
        if !vlan.tcp_forward.is_empty() {
            println!("  TCP forward:    {}", format_forwards(&vlan.tcp_forward));
        }
        if !vlan.udp_forward.is_empty() {
            println!("  UDP forward:    {}", format_forwards(&vlan.udp_forward));
        }
        if let Some(ref dhcp) = vlan.dhcp {
            println!(
                "  DHCP pool:      {}",
                pool_label_v4(&dhcp.pool_start, &dhcp.pool_end)
            );
            println!("  DHCP DNS:       {}", dhcp.dns);
        } else {
            println!("  DHCPv4:         disabled");
        }
        if let Some(ref dhcpv6) = vlan.dhcpv6 {
            println!(
                "  DHCPv6 pool:    {}",
                pool_label_v6(&dhcpv6.pool_start, &dhcpv6.pool_end)
            );
        }
    }

    println!();
    println!("  --- WAN ---");
    println!(
        "  TCP accept:     {}",
        format_ports(&config.wan.tcp_accept)
    );
    println!(
        "  UDP accept:     {}",
        format_ports(&config.wan.udp_accept)
    );
    if !config.wan.tcp_forward.is_empty() {
        println!(
            "  TCP forward:    {}",
            format_forwards(&config.wan.tcp_forward)
        );
    }
    if !config.wan.udp_forward.is_empty() {
        println!(
            "  UDP forward:    {}",
            format_forwards(&config.wan.udp_forward)
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
            if name == "lo" {
                None
            } else {
                Some(name)
            }
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

fn get_iface_driver(iface: &str) -> String {
    let path = format!("/sys/class/net/{iface}/device/driver");
    fs::read_link(&path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "-".to_string())
}

fn get_iface_speed(iface: &str) -> String {
    let path = format!("/sys/class/net/{iface}/speed");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| {
            let v: i64 = s.trim().parse().ok()?;
            if v > 0 {
                Some(format!("{v}Mb/s"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn get_iface_state(iface: &str) -> String {
    let path = format!("/sys/class/net/{iface}/operstate");
    fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn iface_has_ip(iface: &str, ip: &str) -> bool {
    let output = Command::new("ip")
        .args(["-o", "addr", "show", iface])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    output.contains(&format!(" {ip}/"))
}

fn print_interface_table(ifaces: &[String]) {
    let ssh_ip = std::env::var("SSH_CONNECTION")
        .ok()
        .and_then(|c| c.split_whitespace().nth(2).map(String::from));

    println!(
        "  {:<16} {:<19} {:<12} {:<10} {:<6} {}",
        "INTERFACE", "MAC", "DRIVER", "SPEED", "STATE", ""
    );
    println!("  {}", "-".repeat(72));

    for iface in ifaces {
        let mac = get_mac(iface);
        let driver = get_iface_driver(iface);
        let speed = get_iface_speed(iface);
        let state = get_iface_state(iface);
        let is_ssh = ssh_ip
            .as_ref()
            .map_or(false, |ip| iface_has_ip(iface, ip));
        let note = if is_ssh { "<-- SSH" } else { "" };

        println!(
            "  {:<16} {:<19} {:<12} {:<10} {:<6} {}",
            iface, mac, driver, speed, state, note
        );
    }
    println!();
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

fn reset_config() -> Option<HclConfig> {
    println!();
    println!("  This will erase your current configuration and start fresh.");
    println!();
    loop {
        match Text::new("Type 'reset' to confirm, or 'cancel':").prompt() {
            Ok(v) if v.trim().eq_ignore_ascii_case("reset") => break,
            Ok(v) if v.trim().eq_ignore_ascii_case("cancel") => return None,
            Ok(_) => continue,
            Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
                return None
            }
            Err(_) => return None,
        }
    }

    let hostname_re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();

    // Hostname
    println!();
    println!("==> Configure hostname:");
    let hostname = prompt_validated("Hostname for this router", "nifty-filter", |v| {
        if hostname_re.is_match(v) {
            None
        } else {
            Some("Invalid hostname. Must be 1-63 chars: letters, digits, hyphens.")
        }
    })?;

    // Interfaces
    println!();
    println!("==> Configure network interfaces:");
    let ifaces = list_interfaces();
    if ifaces.len() < 2 {
        println!(
            "  Need at least 2 network interfaces (found {}).",
            ifaces.len()
        );
        return None;
    }
    print_interface_table(&ifaces);

    let wan_choice = choose(
        "Select WAN interface (upstream/internet):",
        ifaces.clone(),
        0,
    )?;
    let wan_iface = wan_choice.1;
    println!("  WAN: {wan_iface} -> wan");

    let trunk_ifaces: Vec<String> = ifaces.into_iter().filter(|i| i != &wan_iface).collect();
    let trunk_iface = if trunk_ifaces.len() == 1 {
        println!(
            "  TRUNK: {} -> trunk (only remaining interface)",
            trunk_ifaces[0]
        );
        trunk_ifaces[0].clone()
    } else {
        let choice = choose(
            "Select trunk interface (local network / switch uplink):",
            trunk_ifaces,
            0,
        )?;
        println!("  TRUNK: {} -> trunk", choice.1);
        choice.1
    };

    let wan_mac = get_mac(&wan_iface);
    let trunk_mac = get_mac(&trunk_iface);

    // Subnet
    println!();
    println!("==> Configure LAN network:");
    let vlan_name = prompt_validated("VLAN name", "lan", |v| {
        if v.is_empty() {
            Some("Name cannot be empty.")
        } else if v.contains(' ') || v.contains('"') {
            Some("Name must not contain spaces or quotes.")
        } else {
            None
        }
    })?;

    let subnet_lan = prompt_validated("LAN subnet (IP/prefix)", "10.99.1.1/24", |v| {
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            None
        } else {
            Some("Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).")
        }
    })?;

    let router_ip = subnet_lan
        .split_once('/')
        .map(|(ip, _)| ip)
        .unwrap_or(&subnet_lan)
        .to_string();
    let network_base = router_ip
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(&router_ip)
        .to_string();
    let default_start = format!("{network_base}.100");
    let default_end = format!("{network_base}.250");

    // DHCP
    println!();
    println!("==> Configure DHCP pool:");
    let dhcp_start = prompt_text("DHCP pool start", &default_start)?;
    let dhcp_end = prompt_text("DHCP pool end", &default_end)?;
    let dns_servers = prompt_text("Upstream DNS servers (comma-separated)", "1.1.1.1, 1.0.0.1")?;

    // Build HCL config text
    let hcl_content = format!(
        r#"# nifty-filter configuration
# Edit this file, then apply changes or reboot.

hostname = "{hostname}"

interfaces {{
  trunk {{
    name = "trunk"
    mac  = "{trunk_mac}"
  }}
  wan {{
    name = "wan"
    mac  = "{wan_mac}"
  }}
}}

wan {{
  enable_ipv4 = true
  enable_ipv6 = false

  icmp_accept = []
  tcp_accept  = [22]
  udp_accept  = []
}}

dns {{
  upstream = [{dns_list}]
}}

vlan "{vlan_name}" {{
  id = 1

  ipv4 {{
    subnet = "{subnet}"
    egress = ["0.0.0.0/0"]
  }}

  firewall {{
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22]
    udp_accept  = [53, 67, 68]
  }}

  dhcp {{
    pool_start = "{dhcp_start}"
    pool_end   = "{dhcp_end}"
    router     = "{router_ip}"
    dns        = "{router_ip}"
  }}
}}
"#,
        hostname = hostname,
        trunk_mac = trunk_mac,
        wan_mac = wan_mac,
        dns_list = dns_servers
            .split(',')
            .map(|s| format!("\"{}\"", s.trim()))
            .collect::<Vec<_>>()
            .join(", "),
        vlan_name = vlan_name,
        subnet = subnet_lan,
        dhcp_start = dhcp_start,
        dhcp_end = dhcp_end,
        router_ip = router_ip,
    );

    // Write HCL file
    let config_dir = Path::new(HCL_FILE).parent().unwrap();
    fs::create_dir_all(config_dir).ok();
    fs::write(HCL_FILE, &hcl_content).ok();
    let _ = Command::new("chmod").args(["0600", HCL_FILE]).status();
    let _ = Command::new("sudo")
        .args(["rm", "-f", "/var/lib/dnsmasq/dnsmasq.leases"])
        .status();

    println!();
    println!("  Configuration reset. Apply changes or reboot to activate.");

    // Parse and return the new config
    match parse_hcl(&hcl_content) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("  Warning: could not parse new config: {e}");
            None
        }
    }
}

fn launch_editor(path: &str) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    let _ = Command::new(&editor).arg(path).status();
}

// --- Submenus ---

fn menu_network(config: &mut HclConfig) {
    let mut cursor = 0;
    loop {
        let ipv6_label = if config.wan.enable_ipv6 {
            "Disable WAN IPv6"
        } else {
            "Enable WAN IPv6"
        };

        let mut items = vec![format!(
            "Hostname ({})",
            config.hostname.as_deref().unwrap_or("not set")
        )];
        let mut actions: Vec<MenuAction> = vec![MenuAction::Hostname];

        for (name, vlan) in sorted_vlans(config) {
            let v4_label = vlan
                .ipv4
                .as_ref()
                .map(|v| subnet_label(&v.subnet))
                .unwrap_or_else(|| "not set".to_string());
            items.push(format!("VLAN \"{name}\" (id={}) IPv4 subnet ({v4_label})", vlan.id));
            actions.push(MenuAction::VlanSubnet(name.clone()));

            let v6_label = vlan
                .ipv6
                .as_ref()
                .map(|v| subnet_label(&v.subnet))
                .unwrap_or_else(|| "not set".to_string());
            items.push(format!("VLAN \"{name}\" (id={}) IPv6 subnet ({v6_label})", vlan.id));
            actions.push(MenuAction::VlanSubnetV6(name.clone()));
        }
        items.push(ipv6_label.to_string());
        actions.push(MenuAction::ToggleIpv6);
        items.push("Back".to_string());
        actions.push(MenuAction::Back);

        match choose("Network:", items, cursor) {
            Some((idx, _)) => {
                cursor = idx;
                match &actions[idx] {
                    MenuAction::Hostname => edit_hostname(config),
                    MenuAction::VlanSubnet(name) => edit_vlan_subnet(config, name),
                    MenuAction::VlanSubnetV6(name) => edit_vlan_subnet_ipv6(config, name),
                    MenuAction::ToggleIpv6 => toggle_ipv6(config),
                    MenuAction::Back => break,
                    _ => {}
                }
            }
            None => break,
        }
    }
}

fn menu_firewall(config: &mut HclConfig) {
    let mut cursor = 0;
    loop {
        let mut items = Vec::new();
        let mut actions: Vec<MenuAction> = Vec::new();

        for (name, vlan) in sorted_vlans(config) {
            let tcp = vlan
                .firewall
                .as_ref()
                .map(|f| format_ports(&f.tcp_accept))
                .unwrap_or_default();
            items.push(format!("VLAN \"{name}\" TCP accept ({tcp})"));
            actions.push(MenuAction::VlanTcpAccept(name.clone()));

            let udp = vlan
                .firewall
                .as_ref()
                .map(|f| format_ports(&f.udp_accept))
                .unwrap_or_default();
            items.push(format!("VLAN \"{name}\" UDP accept ({udp})"));
            actions.push(MenuAction::VlanUdpAccept(name.clone()));

            let egress4 = vlan
                .ipv4
                .as_ref()
                .map(|v| format_cidr_list(&v.egress))
                .unwrap_or_default();
            items.push(format!("VLAN \"{name}\" egress IPv4 ({egress4})"));
            actions.push(MenuAction::VlanEgressV4(name.clone()));

            let egress6 = vlan
                .ipv6
                .as_ref()
                .map(|v| format_cidr_list(&v.egress))
                .unwrap_or_default();
            items.push(format!("VLAN \"{name}\" egress IPv6 ({egress6})"));
            actions.push(MenuAction::VlanEgressV6(name.clone()));
        }

        items.push(format!(
            "TCP ports WAN ({})",
            format_ports(&config.wan.tcp_accept)
        ));
        actions.push(MenuAction::WanTcpAccept);
        items.push(format!(
            "UDP ports WAN ({})",
            format_ports(&config.wan.udp_accept)
        ));
        actions.push(MenuAction::WanUdpAccept);
        items.push("Back".to_string());
        actions.push(MenuAction::Back);

        match choose("Firewall:", items, cursor) {
            Some((idx, _)) => {
                cursor = idx;
                match &actions[idx] {
                    MenuAction::VlanTcpAccept(name) => {
                        let name = name.clone();
                        let vlan = config.vlan.get_mut(&name).unwrap();
                        let fw = vlan.firewall.get_or_insert_with(|| FirewallConfig {
                            icmp_accept: vec![],
                            icmpv6_accept: vec![],
                            tcp_accept: vec![],
                            udp_accept: vec![],
                        });
                        edit_ports(
                            &mut fw.tcp_accept,
                            &format!("VLAN \"{name}\" TCP accept"),
                        );
                        save_config(config);
                    }
                    MenuAction::VlanUdpAccept(name) => {
                        let name = name.clone();
                        let vlan = config.vlan.get_mut(&name).unwrap();
                        let fw = vlan.firewall.get_or_insert_with(|| FirewallConfig {
                            icmp_accept: vec![],
                            icmpv6_accept: vec![],
                            tcp_accept: vec![],
                            udp_accept: vec![],
                        });
                        edit_ports(
                            &mut fw.udp_accept,
                            &format!("VLAN \"{name}\" UDP accept"),
                        );
                        save_config(config);
                    }
                    MenuAction::VlanEgressV4(name) => edit_vlan_egress_ipv4(config, name),
                    MenuAction::VlanEgressV6(name) => edit_vlan_egress_ipv6(config, name),
                    MenuAction::WanTcpAccept => {
                        edit_ports(
                            &mut config.wan.tcp_accept,
                            "TCP ports WAN",
                        );
                        save_config(config);
                    }
                    MenuAction::WanUdpAccept => {
                        edit_ports(
                            &mut config.wan.udp_accept,
                            "UDP ports WAN",
                        );
                        save_config(config);
                    }
                    MenuAction::Back => break,
                    _ => {}
                }
            }
            None => break,
        }
    }
}

fn menu_port_forwarding(config: &mut HclConfig) {
    let mut cursor = 0;
    loop {
        let mut items = Vec::new();
        let mut actions: Vec<MenuAction> = Vec::new();

        for (name, vlan) in sorted_vlans(config) {
            items.push(format!(
                "VLAN \"{name}\" TCP forward ({})",
                format_forwards(&vlan.tcp_forward)
            ));
            actions.push(MenuAction::VlanTcpForward(name.clone()));

            items.push(format!(
                "VLAN \"{name}\" UDP forward ({})",
                format_forwards(&vlan.udp_forward)
            ));
            actions.push(MenuAction::VlanUdpForward(name.clone()));
        }

        items.push(format!(
            "TCP forward WAN ({})",
            format_forwards(&config.wan.tcp_forward)
        ));
        actions.push(MenuAction::WanTcpForward);
        items.push(format!(
            "UDP forward WAN ({})",
            format_forwards(&config.wan.udp_forward)
        ));
        actions.push(MenuAction::WanUdpForward);
        items.push("Back".to_string());
        actions.push(MenuAction::Back);

        match choose("Port Forwarding:", items, cursor) {
            Some((idx, _)) => {
                cursor = idx;
                match &actions[idx] {
                    MenuAction::VlanTcpForward(name) => {
                        let name = name.clone();
                        let vlan = config.vlan.get_mut(&name).unwrap();
                        edit_forwards(
                            &mut vlan.tcp_forward,
                            &format!("VLAN \"{name}\" TCP forward"),
                        );
                        save_config(config);
                    }
                    MenuAction::VlanUdpForward(name) => {
                        let name = name.clone();
                        let vlan = config.vlan.get_mut(&name).unwrap();
                        edit_forwards(
                            &mut vlan.udp_forward,
                            &format!("VLAN \"{name}\" UDP forward"),
                        );
                        save_config(config);
                    }
                    MenuAction::WanTcpForward => {
                        edit_forwards(
                            &mut config.wan.tcp_forward,
                            "TCP forward WAN",
                        );
                        save_config(config);
                    }
                    MenuAction::WanUdpForward => {
                        edit_forwards(
                            &mut config.wan.udp_forward,
                            "UDP forward WAN",
                        );
                        save_config(config);
                    }
                    MenuAction::Back => break,
                    _ => {}
                }
            }
            None => break,
        }
    }
}

fn menu_dhcp_dns(config: &mut HclConfig) {
    let mut cursor = 0;
    loop {
        let mut items = Vec::new();
        let mut actions: Vec<MenuAction> = Vec::new();

        for (name, vlan) in sorted_vlans(config) {
            let dhcp_on = vlan.dhcp.is_some();
            let dhcp_label = if dhcp_on {
                format!("VLAN \"{name}\": Disable DHCPv4")
            } else {
                format!("VLAN \"{name}\": Enable DHCPv4")
            };
            items.push(dhcp_label);
            actions.push(MenuAction::ToggleDhcp4(name.clone()));

            if dhcp_on {
                if let Some(ref dhcp) = vlan.dhcp {
                    items.push(format!(
                        "VLAN \"{name}\" DHCP pool ({})",
                        pool_label_v4(&dhcp.pool_start, &dhcp.pool_end)
                    ));
                    actions.push(MenuAction::DhcpPool(name.clone()));

                    items.push(format!(
                        "VLAN \"{name}\" DNS ({})",
                        dhcp.dns
                    ));
                    actions.push(MenuAction::DhcpDns(name.clone()));
                }
            }

            let has_ipv6 = vlan.ipv6.is_some();
            if has_ipv6 {
                let v6_on = vlan.dhcpv6.is_some();
                let v6_label = if v6_on {
                    format!("VLAN \"{name}\": Disable DHCPv6")
                } else {
                    format!("VLAN \"{name}\": Enable DHCPv6")
                };
                items.push(v6_label);
                actions.push(MenuAction::ToggleDhcpv6(name.clone()));

                if let Some(ref dhcpv6) = vlan.dhcpv6 {
                    items.push(format!(
                        "VLAN \"{name}\" DHCPv6 pool ({})",
                        pool_label_v6(&dhcpv6.pool_start, &dhcpv6.pool_end)
                    ));
                    actions.push(MenuAction::Dhcpv6Pool(name.clone()));
                }
            }
        }

        items.push("Back".to_string());
        actions.push(MenuAction::Back);

        match choose("DHCP / DNS:", items, cursor) {
            Some((idx, _)) => {
                cursor = idx;
                match &actions[idx] {
                    MenuAction::ToggleDhcp4(name) => toggle_vlan_dhcp4(config, name),
                    MenuAction::DhcpPool(name) => edit_vlan_dhcp_pool(config, name),
                    MenuAction::DhcpDns(name) => edit_vlan_dns(config, name),
                    MenuAction::ToggleDhcpv6(name) => toggle_vlan_dhcpv6(config, name),
                    MenuAction::Dhcpv6Pool(name) => edit_vlan_dhcpv6_pool(config, name),
                    MenuAction::Back => break,
                    _ => {}
                }
            }
            None => break,
        }
    }
}

// Menu action enum for clean dispatch
enum MenuAction {
    Hostname,
    VlanSubnet(String),
    VlanSubnetV6(String),
    ToggleIpv6,
    VlanTcpAccept(String),
    VlanUdpAccept(String),
    VlanEgressV4(String),
    VlanEgressV6(String),
    WanTcpAccept,
    WanUdpAccept,
    VlanTcpForward(String),
    VlanUdpForward(String),
    WanTcpForward,
    WanUdpForward,
    ToggleDhcp4(String),
    DhcpPool(String),
    DhcpDns(String),
    ToggleDhcpv6(String),
    Dhcpv6Pool(String),
    Back,
}

// --- Main menu ---

pub fn run() {
    let mut config = match hcl_file::load(Path::new(HCL_FILE)) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };

    let mut cursor = 0;
    loop {
        println!();

        let items = vec![
            "Show config".to_string(),
            "Show status".to_string(),
            "Show logs".to_string(),
            "Network".to_string(),
            "Firewall".to_string(),
            "Port forwarding".to_string(),
            "DHCP / DNS".to_string(),
            "Apply changes".to_string(),
            "Edit nifty-filter.hcl".to_string(),
            "Reset config".to_string(),
            "Reboot".to_string(),
            "Quit".to_string(),
        ];

        match choose("nifty-filter configuration:", items, cursor) {
            Some((idx, choice)) => {
                cursor = idx;
                match choice.as_str() {
                    "Show config" => show_config(&config),
                    "Show status" => show_status(),
                    "Show logs" => menu_logs(),
                    "Network" => menu_network(&mut config),
                    "Firewall" => menu_firewall(&mut config),
                    "Port forwarding" => menu_port_forwarding(&mut config),
                    "DHCP / DNS" => menu_dhcp_dns(&mut config),
                    "Apply changes" => apply_changes(&config),
                    "Edit nifty-filter.hcl" => {
                        launch_editor(HCL_FILE);
                        match hcl_file::load(Path::new(HCL_FILE)) {
                            Ok(new_config) => config = new_config,
                            Err(e) => eprintln!("  Warning: {e}"),
                        }
                    }
                    "Reset config" => {
                        if let Some(new_config) = reset_config() {
                            config = new_config;
                        }
                    }
                    "Reboot" => {
                        if let Some(v) = prompt_text("Reboot now? (yes/no)", "no") {
                            if v.trim().eq_ignore_ascii_case("yes")
                                || v.trim().eq_ignore_ascii_case("y")
                            {
                                let _ = Command::new("sudo")
                                    .args(["systemctl", "reboot"])
                                    .status();
                            }
                        }
                    }
                    "Quit" => break,
                    _ => {}
                }
            }
            None => break, // ESC at main menu = quit
        }
    }
}
