use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;

use inquire::{InquireError, Select, Text};
use ipnetwork::IpNetwork;
use regex::Regex;

use super::env_file::EnvFile;

const ENV_FILE: &str = "/var/nifty-filter/nifty-filter.env";

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

/// Get the list of configured VLAN IDs from the env file.
/// Returns IDs from VLANS= if set, otherwise auto-detects from VLAN_N_* keys, else [1].
fn get_vlan_ids(env: &EnvFile) -> Vec<u16> {
    let vlans_str = env.get("VLANS").to_string();
    if !vlans_str.is_empty() {
        return vlans_str
            .split(',')
            .filter_map(|s| s.trim().parse::<u16>().ok())
            .collect();
    }
    // Auto-detect from VLAN_N_* keys
    let mut ids: Vec<u16> = env
        .keys()
        .filter_map(|k| {
            if k.starts_with("VLAN_") && k.len() > 5 {
                k[5..].split('_').next()?.parse::<u16>().ok()
            } else {
                None
            }
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    ids.sort();
    if ids.is_empty() {
        vec![1]
    } else {
        ids
    }
}

/// Get a per-VLAN env key name, e.g., vlan_key(10, "SUBNET_IPV4") -> "VLAN_10_SUBNET_IPV4"
fn vlan_key(id: u16, suffix: &str) -> String {
    format!("VLAN_{}_{}", id, suffix)
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

fn edit_vlan_subnet(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "SUBNET_IPV4");
    let current = env.get(&key).to_string();
    let default = if current.is_empty() {
        "10.99.1.1/24".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(&format!("VLAN {vid} IPv4 subnet (IP/prefix)"), &default) {
            Some(v) => v,
            None => return,
        };
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).");
    };
    env.set(&key, &val);

    // Update DHCP defaults to match
    if let Some((router_ip, _)) = val.split_once('/') {
        if let Some(base) = router_ip.rsplit_once('.') {
            env.set(&vlan_key(vid, "DHCP_ROUTER"), router_ip);
            env.set(&vlan_key(vid, "DHCP_DNS"), router_ip);
            env.set(&vlan_key(vid, "DHCP_POOL_START"), &format!("{}.100", base.0));
            env.set(&vlan_key(vid, "DHCP_POOL_END"), &format!("{}.250", base.0));
            println!("  Updated VLAN {vid} DHCP pool to match.");
        }
    }
    env.save().ok();
    println!("  Set {key}={val}");
}

fn edit_vlan_subnet_ipv6(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "SUBNET_IPV6");
    let current = env.get(&key).to_string();
    let default = if current.is_empty() {
        "fd00:10::1/64".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(&format!("VLAN {vid} IPv6 subnet (IP/prefix)"), &default) {
            Some(v) => v,
            None => return,
        };
        if v.contains('/') && v.parse::<IpNetwork>().is_ok() {
            break v;
        }
        println!("  Invalid subnet. Use CIDR notation (e.g. fd00:10::1/64).");
    };
    env.set(&key, &val);

    // Update DHCPv6 pool to match if enabled
    if env.get(&vlan_key(vid, "DHCPV6_ENABLED")) == "true" {
        if let Some((addr, _)) = val.split_once('/') {
            if let Some((prefix, _)) = addr.rsplit_once(':') {
                env.set(&vlan_key(vid, "DHCPV6_POOL_START"), &format!("{prefix}:100"));
                env.set(&vlan_key(vid, "DHCPV6_POOL_END"), &format!("{prefix}:1ff"));
                println!("  Updated VLAN {vid} DHCPv6 pool to match.");
            }
        }
    }
    env.save().ok();
    println!("  Set {key}={val}");
}

fn toggle_ipv6(env: &mut EnvFile) {
    if env.get("WAN_ENABLE_IPV6") == "true" {
        env.set("WAN_ENABLE_IPV6", "false");
        env.save().ok();
        println!("  IPv6 disabled.");
    } else {
        env.set("WAN_ENABLE_IPV6", "true");
        env.save().ok();
        println!("  IPv6 enabled. Configure per-VLAN IPv6 subnets in the Network menu.");
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

fn edit_vlan_egress_ipv4(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "EGRESS_ALLOWED_IPV4");
    let current = env.get(&key).to_string();
    let default = if current.is_empty() {
        "0.0.0.0/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(&format!("VLAN {vid} allowed IPv4 egress CIDRs"), &default) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like 0.0.0.0/0 or 10.0.0.0/8,172.16.0.0/12.");
    };
    env.set(&key, &val);
    env.save().ok();
    println!("  Set {key}={val}");
}

fn edit_vlan_egress_ipv6(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "EGRESS_ALLOWED_IPV6");
    let current = env.get(&key).to_string();
    let default = if current.is_empty() {
        "::/0".to_string()
    } else {
        current
    };
    let val = loop {
        let v = match prompt_text(&format!("VLAN {vid} allowed IPv6 egress CIDRs"), &default) {
            Some(v) => v,
            None => return,
        };
        if validate_cidr_list(&v) {
            break v;
        }
        println!("  Invalid CIDR(s). Use notation like ::/0 or fd00::/8.");
    };
    env.set(&key, &val);
    env.save().ok();
    println!("  Set {key}={val}");
}

fn edit_vlan_dhcp_pool(env: &mut EnvFile, vid: u16) {
    let sk = vlan_key(vid, "DHCP_POOL_START");
    let ek = vlan_key(vid, "DHCP_POOL_END");
    let start = env.get(&sk).to_string();
    let end = env.get(&ek).to_string();
    let start = match prompt_text(&format!("VLAN {vid} DHCP pool start"), &start) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text(&format!("VLAN {vid} DHCP pool end"), &end) {
        Some(v) => v,
        None => return,
    };
    env.set(&sk, &start);
    env.set(&ek, &end);
    env.save().ok();
    println!("  Set VLAN {vid} pool: {start} - {end}");
}

fn edit_vlan_dns(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "DHCP_DNS");
    let current = env.get(&key).to_string();
    let val = match prompt_text(&format!("VLAN {vid} DNS servers (comma-separated)"), &current) {
        Some(v) => v,
        None => return,
    };
    env.set(&key, &val);
    env.save().ok();
    println!("  Set {key}={val}");
}

fn toggle_vlan_dhcp4(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "DHCP_ENABLED");
    let udp_key = vlan_key(vid, "UDP_ACCEPT");
    let enabled = env.get(&key);
    let currently_enabled = enabled.is_empty() || enabled == "true";
    if currently_enabled {
        env.set(&key, "false");
        let udp = env.get(&udp_key).to_string();
        let cleaned = udp
            .replace(",67,68", "")
            .replace("67,68,", "")
            .replace("67,68", "");
        env.set(&udp_key, &cleaned);
        env.save().ok();
        println!("  VLAN {vid} DHCPv4 disabled.");
    } else {
        env.set(&key, "true");
        let udp = env.get(&udp_key).to_string();
        if !udp.contains("67") {
            let new_val = if udp.is_empty() {
                "67,68".to_string()
            } else {
                format!("{udp},67,68")
            };
            env.set(&udp_key, &new_val);
        }
        env.save().ok();
        println!("  VLAN {vid} DHCPv4 enabled.");
    }
}

fn toggle_vlan_dhcpv6(env: &mut EnvFile, vid: u16) {
    let key = vlan_key(vid, "DHCPV6_ENABLED");
    let udp_key = vlan_key(vid, "UDP_ACCEPT");
    if env.get(&key) == "true" {
        env.set(&key, "false");
        let udp = env.get(&udp_key).to_string();
        let cleaned = udp
            .replace(",546,547", "")
            .replace("546,547,", "")
            .replace("546,547", "");
        env.set(&udp_key, &cleaned);
        env.save().ok();
        println!("  VLAN {vid} DHCPv6 disabled.");
    } else {
        env.set(&key, "true");
        let udp = env.get(&udp_key).to_string();
        if !udp.contains("546") {
            let new_val = if udp.is_empty() {
                "546,547".to_string()
            } else {
                format!("{udp},546,547")
            };
            env.set(&udp_key, &new_val);
        }
        env.save().ok();
        let pool_key = vlan_key(vid, "DHCPV6_POOL_START");
        if env.get(&pool_key).is_empty() {
            println!("  VLAN {vid} DHCPv6 requires a pool range.");
            edit_vlan_dhcpv6_pool(env, vid);
        }
        println!("  VLAN {vid} DHCPv6 enabled.");
    }
}

fn edit_vlan_dhcpv6_pool(env: &mut EnvFile, vid: u16) {
    let sk = vlan_key(vid, "DHCPV6_POOL_START");
    let ek = vlan_key(vid, "DHCPV6_POOL_END");
    let mut start = env.get(&sk).to_string();
    let mut end = env.get(&ek).to_string();
    if start.is_empty() {
        let subnet = env.get(&vlan_key(vid, "SUBNET_IPV6"));
        if let Some((addr, _)) = subnet.split_once('/') {
            if let Some(prefix) = addr.rsplit_once(':') {
                start = format!("{}:100", prefix.0);
                end = format!("{}:1ff", prefix.0);
            }
        }
    }
    let start = match prompt_text(&format!("VLAN {vid} DHCPv6 pool start"), &start) {
        Some(v) => v,
        None => return,
    };
    let end = match prompt_text(&format!("VLAN {vid} DHCPv6 pool end"), &end) {
        Some(v) => v,
        None => return,
    };
    env.set(&sk, &start);
    env.set(&ek, &end);
    env.save().ok();
    println!("  Set VLAN {vid} DHCPv6 pool: {start} - {end}");
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
    let _ = Command::new("sudo").args(["hostname", hostname]).status();
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
            Some((_, choice)) if choice == "Back" => break,
            Some((idx, choice)) if choice == "All (this boot)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-b"]));
                println!();
            }
            Some((idx, choice)) if choice == "All (last boot)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-b", "-1"]));
                println!();
            }
            Some((idx, choice)) if choice == "All (live)" => {
                cursor = idx;
                run_interactive(Command::new("journalctl").args(["-f"]));
                println!();
            }
            Some((idx, _choice)) => {
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

fn show_config(env: &EnvFile) {
    println!();
    println!("  === Current Configuration ===");
    println!("  ENABLED:        {}", env.get("ENABLED"));
    println!("  HOSTNAME:       {}", env.get("HOSTNAME"));
    println!("  INTERFACE_WAN:  {} ({})", env.get("INTERFACE_WAN"), env.get("WAN_MAC"));
    let trunk = env.get("INTERFACE_TRUNK");
    let trunk_display = if trunk.is_empty() { env.get("INTERFACE_LAN") } else { trunk };
    println!("  INTERFACE_TRUNK: {} ({})", trunk_display, env.get("TRUNK_MAC"));
    let mgmt_mac = env.get("MGMT_MAC");
    if !mgmt_mac.is_empty() {
        println!("  INTERFACE_MGMT: {} ({})", env.get("INTERFACE_MGMT"), mgmt_mac);
    }
    println!(
        "  ENABLE_IPV4:    {}",
        if env.get("WAN_ENABLE_IPV4").is_empty() { "true" } else { env.get("WAN_ENABLE_IPV4") }
    );
    println!(
        "  ENABLE_IPV6:    {}",
        if env.get("WAN_ENABLE_IPV6").is_empty() { "false" } else { env.get("WAN_ENABLE_IPV6") }
    );
    let switch_aware = env.get("VLAN_AWARE_SWITCH") == "true";
    println!("  SWITCH MODE:    {}", if switch_aware { "VLAN-aware" } else { "simple" });

    let vids = get_vlan_ids(env);
    for vid in &vids {
        let v4 = env.get(&vlan_key(*vid, "SUBNET_IPV4"));
        let v6 = env.get(&vlan_key(*vid, "SUBNET_IPV6"));
        let egress = env.get(&vlan_key(*vid, "EGRESS_ALLOWED_IPV4"));
        let tcp = env.get(&vlan_key(*vid, "TCP_ACCEPT"));
        let udp = env.get(&vlan_key(*vid, "UDP_ACCEPT"));
        let egress_label = if egress.is_empty() { "deny" } else { egress };
        let vlan_name = env.get(&vlan_key(*vid, "NAME"));
        println!();
        if vlan_name.is_empty() {
            println!("  --- VLAN {vid} ---");
        } else {
            println!("  --- VLAN {vid} ({vlan_name}) ---");
        }
        if !v4.is_empty() { println!("  Subnet IPv4:    {}", subnet_label(v4)); }
        if !v6.is_empty() { println!("  Subnet IPv6:    {}", subnet_label(v6)); }
        println!("  Egress IPv4:    {egress_label}");
        println!("  TCP accept:     {tcp}");
        println!("  UDP accept:     {udp}");
        println!("  TCP forward:    {}", env.get(&vlan_key(*vid, "TCP_FORWARD")));
        println!("  UDP forward:    {}", env.get(&vlan_key(*vid, "UDP_FORWARD")));
        let dhcp_en = env.get(&vlan_key(*vid, "DHCP_ENABLED"));
        let dhcp_on = dhcp_en.is_empty() || dhcp_en == "true";
        if dhcp_on {
            println!(
                "  DHCP pool:      {}",
                pool_label_v4(
                    env.get(&vlan_key(*vid, "DHCP_POOL_START")),
                    env.get(&vlan_key(*vid, "DHCP_POOL_END"))
                )
            );
            println!("  DHCP DNS:       {}", env.get(&vlan_key(*vid, "DHCP_DNS")));
        } else {
            println!("  DHCPv4:         disabled");
        }
        if env.get(&vlan_key(*vid, "DHCPV6_ENABLED")) == "true" {
            println!(
                "  DHCPv6 pool:    {}",
                pool_label_v6(
                    env.get(&vlan_key(*vid, "DHCPV6_POOL_START")),
                    env.get(&vlan_key(*vid, "DHCPV6_POOL_END"))
                )
            );
        }
    }

    println!();
    println!("  --- WAN ---");
    println!("  TCP_ACCEPT_WAN: {}", env.get("TCP_ACCEPT_WAN"));
    println!("  UDP_ACCEPT_WAN: {}", env.get("UDP_ACCEPT_WAN"));
    println!("  TCP_FORWARD_WAN: {}", env.get("TCP_FORWARD_WAN"));
    println!("  UDP_FORWARD_WAN: {}", env.get("UDP_FORWARD_WAN"));
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
            if v > 0 { Some(format!("{v}Mb/s")) } else { None }
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

fn reset_config() -> Option<EnvFile> {
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
        println!("  TRUNK: {} -> trunk (only remaining interface)", trunk_ifaces[0]);
        trunk_ifaces[0].clone()
    } else {
        let choice = choose("Select trunk interface (local network / switch uplink):", trunk_ifaces, 0)?;
        println!("  TRUNK: {} -> trunk", choice.1);
        choice.1
    };

    let wan_mac = get_mac(&wan_iface);
    let trunk_mac = get_mac(&trunk_iface);

    // Subnet
    println!();
    println!("==> Configure LAN network (VLAN 1):");
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

    // Write env file
    let env_content = format!(
        r#"# nifty-filter configuration
# Edit this file and run: nifty-config -> Apply changes
ENABLED=true
HOSTNAME={hostname}

# Network interfaces
INTERFACE_TRUNK=trunk
INTERFACE_WAN=wan
VLAN_AWARE_SWITCH=false

# ICMP/ports accepted on WAN
ICMP_ACCEPT_WAN=
TCP_ACCEPT_WAN=22
UDP_ACCEPT_WAN=

# WAN port forwarding
TCP_FORWARD_WAN=
UDP_FORWARD_WAN=

# VLAN 1 (default LAN)
VLAN_1_SUBNET_IPV4={subnet}
VLAN_1_EGRESS_ALLOWED_IPV4=0.0.0.0/0
VLAN_1_ICMP_ACCEPT=echo-request,echo-reply,destination-unreachable,time-exceeded
VLAN_1_TCP_ACCEPT=22
VLAN_1_UDP_ACCEPT=67,68
VLAN_1_TCP_FORWARD=
VLAN_1_UDP_FORWARD=

# DHCP for VLAN 1
VLAN_1_DHCP_ENABLED=true
VLAN_1_DHCP_POOL_START={dhcp_start}
VLAN_1_DHCP_POOL_END={dhcp_end}
VLAN_1_DHCP_ROUTER={router_ip}
VLAN_1_DHCP_DNS={router_ip}
VLAN_1_DHCPV6_ENABLED=false
VLAN_1_DHCPV6_POOL_START=
VLAN_1_DHCPV6_POOL_END=

# Upstream DNS
DHCP_UPSTREAM_DNS={dns}
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
    )
    .ok();
    fs::write(
        format!("{network_dir}/10-trunk.link"),
        format!("[Match]\nMACAddress={trunk_mac}\n\n[Link]\nName=trunk\n"),
    )
    .ok();

    fs::write(ENV_FILE, &env_content).ok();
    let _ = Command::new("chmod").args(["0600", ENV_FILE]).status();
    let _ = Command::new("sudo")
        .args(["rm", "-f", "/var/lib/dnsmasq/dnsmasq.leases"])
        .status();

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
        let ipv6_enabled = env.get("WAN_ENABLE_IPV6") == "true";
        let ipv6_label = if ipv6_enabled { "Disable WAN IPv6" } else { "Enable WAN IPv6" };
        let vids = get_vlan_ids(env);

        let mut items = vec![format!("Hostname ({})", env.get("HOSTNAME"))];
        for vid in &vids {
            items.push(format!(
                "VLAN {vid} IPv4 subnet ({})",
                subnet_label(env.get(&vlan_key(*vid, "SUBNET_IPV4")))
            ));
            items.push(format!(
                "VLAN {vid} IPv6 subnet ({})",
                subnet_label(env.get(&vlan_key(*vid, "SUBNET_IPV6")))
            ));
        }
        items.push(ipv6_label.to_string());
        items.push("Back".to_string());

        match choose("Network:", items, cursor) {
            Some((idx, choice)) if choice.starts_with("Hostname") => {
                cursor = idx;
                edit_hostname(env)
            }
            Some((idx, choice)) if choice.contains("IPv4 subnet") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_subnet(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("IPv6 subnet") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_subnet_ipv6(env, vid);
                }
            }
            Some((idx, choice)) if choice == "Enable WAN IPv6" || choice == "Disable WAN IPv6" => {
                cursor = idx;
                toggle_ipv6(env)
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_firewall(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let vids = get_vlan_ids(env);

        let mut items = Vec::new();
        for vid in &vids {
            items.push(format!(
                "VLAN {vid} TCP accept ({})",
                env.get(&vlan_key(*vid, "TCP_ACCEPT"))
            ));
            items.push(format!(
                "VLAN {vid} UDP accept ({})",
                env.get(&vlan_key(*vid, "UDP_ACCEPT"))
            ));
            items.push(format!(
                "VLAN {vid} egress IPv4 ({})",
                env.get(&vlan_key(*vid, "EGRESS_ALLOWED_IPV4"))
            ));
            items.push(format!(
                "VLAN {vid} egress IPv6 ({})",
                env.get(&vlan_key(*vid, "EGRESS_ALLOWED_IPV6"))
            ));
        }
        items.push(format!("TCP ports WAN ({})", env.get("TCP_ACCEPT_WAN")));
        items.push(format!("UDP ports WAN ({})", env.get("UDP_ACCEPT_WAN")));
        items.push("Back".to_string());

        match choose("Firewall:", items, cursor) {
            Some((idx, choice)) if choice.contains("TCP accept") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_ports(env, &vlan_key(vid, "TCP_ACCEPT"), &format!("VLAN {vid} TCP accept"));
                }
            }
            Some((idx, choice)) if choice.contains("UDP accept") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_ports(env, &vlan_key(vid, "UDP_ACCEPT"), &format!("VLAN {vid} UDP accept"));
                }
            }
            Some((idx, choice)) if choice.contains("egress IPv4") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_egress_ipv4(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("egress IPv6") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_egress_ipv6(env, vid);
                }
            }
            Some((idx, choice)) if choice.starts_with("TCP ports WAN") => {
                cursor = idx;
                edit_ports(env, "TCP_ACCEPT_WAN", "TCP ports WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP ports WAN") => {
                cursor = idx;
                edit_ports(env, "UDP_ACCEPT_WAN", "UDP ports WAN")
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_port_forwarding(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let vids = get_vlan_ids(env);
        let mut items = Vec::new();
        for vid in &vids {
            items.push(format!(
                "VLAN {vid} TCP forward ({})",
                env.get(&vlan_key(*vid, "TCP_FORWARD"))
            ));
            items.push(format!(
                "VLAN {vid} UDP forward ({})",
                env.get(&vlan_key(*vid, "UDP_FORWARD"))
            ));
        }
        items.push(format!("TCP forward WAN ({})", env.get("TCP_FORWARD_WAN")));
        items.push(format!("UDP forward WAN ({})", env.get("UDP_FORWARD_WAN")));
        items.push("Back".to_string());

        match choose("Port Forwarding:", items, cursor) {
            Some((idx, choice)) if choice.contains("TCP forward") && choice.starts_with("VLAN") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_forwards(env, &vlan_key(vid, "TCP_FORWARD"), &format!("VLAN {vid} TCP forward"));
                }
            }
            Some((idx, choice)) if choice.contains("UDP forward") && choice.starts_with("VLAN") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_forwards(env, &vlan_key(vid, "UDP_FORWARD"), &format!("VLAN {vid} UDP forward"));
                }
            }
            Some((idx, choice)) if choice.starts_with("TCP forward WAN") => {
                cursor = idx;
                edit_forwards(env, "TCP_FORWARD_WAN", "TCP forward WAN")
            }
            Some((idx, choice)) if choice.starts_with("UDP forward WAN") => {
                cursor = idx;
                edit_forwards(env, "UDP_FORWARD_WAN", "UDP forward WAN")
            }
            Some((_, choice)) if choice == "Back" => break,
            _ => break,
        }
    }
}

fn menu_dhcp_dns(env: &mut EnvFile) {
    let mut cursor = 0;
    loop {
        let vids = get_vlan_ids(env);

        let mut items = Vec::new();
        for vid in &vids {
            let dhcp_val = env.get(&vlan_key(*vid, "DHCP_ENABLED"));
            let dhcp_on = dhcp_val.is_empty() || dhcp_val == "true";
            let dhcp_label = if dhcp_on {
                format!("VLAN {vid}: Disable DHCPv4")
            } else {
                format!("VLAN {vid}: Enable DHCPv4")
            };
            items.push(dhcp_label);

            if dhcp_on {
                items.push(format!(
                    "VLAN {vid} DHCP pool ({})",
                    pool_label_v4(
                        env.get(&vlan_key(*vid, "DHCP_POOL_START")),
                        env.get(&vlan_key(*vid, "DHCP_POOL_END"))
                    )
                ));
                items.push(format!(
                    "VLAN {vid} DNS ({})",
                    env.get(&vlan_key(*vid, "DHCP_DNS"))
                ));
            }

            let vlan_has_ipv6 = !env.get(&vlan_key(*vid, "SUBNET_IPV6")).is_empty();
            if vlan_has_ipv6 {
                let v6_on = env.get(&vlan_key(*vid, "DHCPV6_ENABLED")) == "true";
                let v6_label = if v6_on {
                    format!("VLAN {vid}: Disable DHCPv6")
                } else {
                    format!("VLAN {vid}: Enable DHCPv6")
                };
                items.push(v6_label);
                if v6_on {
                    items.push(format!(
                        "VLAN {vid} DHCPv6 pool ({})",
                        pool_label_v6(
                            env.get(&vlan_key(*vid, "DHCPV6_POOL_START")),
                            env.get(&vlan_key(*vid, "DHCPV6_POOL_END"))
                        )
                    ));
                }
            }
        }
        items.push("Back".to_string());

        match choose("DHCP / DNS:", items, cursor) {
            Some((idx, choice)) if choice.contains("Enable DHCPv4") || choice.contains("Disable DHCPv4") => {
                cursor = idx;
                if let Some(vid) = choice.split(':').next().and_then(|s| s.split_whitespace().nth(1)).and_then(|s| s.parse::<u16>().ok()) {
                    toggle_vlan_dhcp4(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("DHCP pool") && !choice.contains("v6") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_dhcp_pool(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("DNS (") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_dns(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("Enable DHCPv6") || choice.contains("Disable DHCPv6") => {
                cursor = idx;
                if let Some(vid) = choice.split(':').next().and_then(|s| s.split_whitespace().nth(1)).and_then(|s| s.parse::<u16>().ok()) {
                    toggle_vlan_dhcpv6(env, vid);
                }
            }
            Some((idx, choice)) if choice.contains("DHCPv6 pool") => {
                cursor = idx;
                if let Some(vid) = choice.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()) {
                    edit_vlan_dhcpv6_pool(env, vid);
                }
            }
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
            Some((idx, choice)) => {
                cursor = idx;
                match choice.as_str() {
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
                            if v.trim().eq_ignore_ascii_case("yes")
                                || v.trim().eq_ignore_ascii_case("y")
                            {
                                let _ = Command::new("sudo").args(["systemctl", "reboot"]).status();
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
