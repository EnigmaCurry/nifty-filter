use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{self, Command, Stdio};

use inquire::{InquireError, Select, Text};
use ipnetwork::IpNetwork;
use regex::Regex;

const AUTH_KEYS: &str = "/home/admin/.ssh/authorized_keys";

fn die(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    process::exit(1);
}

fn run_cmd(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_cmd_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn prompt_text_validated<F>(message: &str, default: &str, validate: F) -> String
where
    F: Fn(&str) -> Option<&'static str>,
{
    loop {
        let mut prompt = Text::new(message);
        if !default.is_empty() {
            prompt = prompt.with_default(default);
        }
        match prompt.prompt() {
            Ok(val) => {
                if let Some(err) = validate(&val) {
                    println!("  {err}");
                    continue;
                }
                return val;
            }
            Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
                println!("Aborted.");
                process::exit(1);
            }
            Err(e) => die(&format!("Prompt error: {e}")),
        }
    }
}

fn prompt_select(message: &str, options: Vec<String>) -> String {
    match Select::new(message, options).prompt() {
        Ok(choice) => choice,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            println!("Aborted.");
            process::exit(1);
        }
        Err(e) => die(&format!("Prompt error: {e}")),
    }
}

fn prompt_confirm(message: &str) -> bool {
    loop {
        match Text::new(&format!("{} (yes/no)", message)).prompt() {
            Ok(answer) => match answer.trim().to_lowercase().as_str() {
                "yes" | "y" => return true,
                "no" | "n" => return false,
                _ => continue,
            },
            Err(InquireError::OperationInterrupted) => {
                eprintln!("Aborted.");
                std::process::exit(1);
            }
            Err(_) => continue,
        }
    }
}

// --- Pre-flight checks ---

fn check_ssh_auth() {
    let ssh_conn = std::env::var("SSH_CONNECTION").unwrap_or_default();
    if ssh_conn.is_empty() {
        return; // console is fine
    }

    // Find sshd PID for this session
    let mut pid = process::id();
    loop {
        let stat = format!("/proc/{pid}/stat");
        let content = match fs::read_to_string(&stat) {
            Ok(c) => c,
            Err(_) => break,
        };
        // stat format: pid (comm) state ppid ...
        let ppid: u32 = content
            .rsplit_once(") ")
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        if ppid <= 1 {
            break;
        }
        let comm = fs::read_to_string(format!("/proc/{ppid}/comm"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if comm == "sshd" {
            // Check auth method from journal
            let output = run_cmd_output(
                "journalctl",
                &[&format!("_PID={ppid}"), "-o", "cat", "--no-pager"],
            );
            if output.contains("Accepted password") {
                println!();
                println!("REFUSED: You are connected via password authentication.");
                println!();
                println!("The installer requires SSH key authentication so that");
                println!("trust is established before writing to disk.");
                println!();
                println!("Steps:");
                println!("  1. Add your public key (from your workstation):");
                println!("       ssh-copy-id admin@<this-host>");
                println!("  2. Disconnect and reconnect with your key:");
                println!("       ssh admin@<this-host>");
                println!("  3. Run this installer again");
                println!();
                process::exit(1);
            }
            break;
        }
        pid = ppid;
    }
}

fn check_authorized_keys() {
    let path = Path::new(AUTH_KEYS);
    if !path.exists() || fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true) {
        println!();
        println!("REFUSED: No SSH authorized keys found.");
        println!();
        println!("You must add at least one SSH public key before installing.");
        println!("This key will be carried into the installed system.");
        println!();
        println!("  ssh-copy-id admin@<this-host>");
        println!();
        println!("Then run this installer again.");
        println!();
        process::exit(1);
    }
}

fn show_authorized_keys() {
    println!();
    println!("==> Authorized keys that will be installed:");
    if let Ok(file) = fs::File::open(AUTH_KEYS) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let label = parts.last().unwrap_or(&"");
                println!("  {} {label}", parts[0]);
            }
        }
    }
    println!();
}

// --- System discovery ---

fn list_disks() -> Vec<(String, String)> {
    // Returns (name, description) pairs
    let output = run_cmd_output("lsblk", &["-ndo", "NAME,SIZE,MODEL", "-e", "7,11"]);
    output
        .lines()
        .filter(|l| !l.contains("loop"))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                return None;
            }
            let name = parts[0].to_string();
            let desc = parts[1..].join(" ");
            Some((name, desc))
        })
        .collect()
}

fn list_interfaces() -> Vec<String> {
    let output = run_cmd_output("ip", &["-o", "link", "show"]);
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
    let output = run_cmd_output("ip", &["-o", "link", "show", iface]);
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
    let output = run_cmd_output("ip", &["-o", "addr", "show", iface]);
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

// --- Interactive prompts ---

struct VlanInstallConfig {
    id: u16,
    name: String,
    subnet: String,
    router_ip: String,
    egress: bool,
    enable_ipv6: bool,
    subnet_ipv6: String,
    dhcp_start: String,
    dhcp_end: String,
}

struct InstallConfig {
    hostname: String,
    disk: String,
    wan_iface: String,
    trunk_iface: String,
    wan_mac: String,
    trunk_mac: String,
    mgmt_iface: Option<String>,
    mgmt_mac: Option<String>,
    subnet_mgmt: Option<String>,
    extra_ifaces: Vec<(String, String, String)>, // (original, new_name, mac)
    wan_enable_ipv6: bool,
    vlan_aware_switch: bool,
    vlans: Vec<VlanInstallConfig>,
    dns_servers: String,
    git_remote: Option<String>,
}

fn prompt_vlan_config(vlan_id: u16, default_subnet_base: &str, used_names: &mut Vec<String>) -> VlanInstallConfig {
    let iface_name_re = Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{0,14}$").unwrap();
    let name = prompt_text_validated(
        &format!("VLAN {vlan_id} interface name"),
        "",
        |v| {
            if !iface_name_re.is_match(v) {
                return Some("Invalid name. Use 1-15 chars: letters, digits, hyphens, underscores.");
            }
            if used_names.iter().any(|n| n == v) {
                return Some("Name already in use.");
            }
            None
        },
    );
    used_names.push(name.clone());

    let default_subnet = format!("{default_subnet_base}.1/24");
    let subnet = prompt_text_validated(
        &format!("VLAN {vlan_id} subnet (router IP/prefix)"),
        &default_subnet,
        |v| {
            if v.parse::<IpNetwork>().is_ok() {
                None
            } else {
                Some("Invalid subnet. Use CIDR notation (e.g. 10.10.0.1/24).")
            }
        },
    );
    let router_ip = subnet
        .split_once('/')
        .map(|(ip, _)| ip)
        .unwrap_or(&subnet)
        .to_string();
    let network_base = router_ip
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(&router_ip)
        .to_string();
    let dhcp_start = format!("{network_base}.100");
    let dhcp_end = format!("{network_base}.250");

    let egress = prompt_confirm(&format!("VLAN {vlan_id}: allow internet access?"));

    let enable_ipv6 = prompt_confirm(&format!("VLAN {vlan_id}: enable IPv6?"));
    let subnet_ipv6 = if enable_ipv6 {
        let default_v6 = format!("fd00:{vlan_id:x}::1/64");
        let v6 = prompt_text_validated(
            &format!("VLAN {vlan_id} IPv6 subnet (router IP/prefix)"),
            &default_v6,
            |v| {
                if v.parse::<IpNetwork>().is_ok() {
                    None
                } else {
                    Some("Invalid subnet. Use CIDR notation (e.g. fd00:10::1/64).")
                }
            },
        );
        v6
    } else {
        String::new()
    };

    println!(
        "  VLAN {vlan_id} ({name}): {subnet} (internet: {}, IPv6: {})",
        if egress { "yes" } else { "no" },
        if enable_ipv6 { "yes" } else { "no" }
    );

    VlanInstallConfig {
        id: vlan_id,
        name,
        subnet,
        router_ip,
        egress,
        enable_ipv6,
        subnet_ipv6,
        dhcp_start,
        dhcp_end,
    }
}

fn gather_config(git_remote: Option<String>) -> InstallConfig {
    let hostname_re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();

    // Hostname
    println!("==> Configure hostname:");
    let hostname = prompt_text_validated("Hostname for this router", "nifty-filter", |v| {
        if hostname_re.is_match(v) {
            None
        } else {
            Some("Invalid hostname. Must be 1-63 chars: letters, digits, hyphens.")
        }
    });
    println!("  Hostname: {hostname}");
    println!();

    // Disk selection
    println!("==> Select target disk for installation:");
    let disks = list_disks();
    if disks.is_empty() {
        die("No disks found");
    }
    let disk = if disks.len() == 1 {
        println!("  Only one disk found: {} ({})", disks[0].0, disks[0].1);
        disks[0].0.clone()
    } else {
        let options: Vec<String> = disks
            .iter()
            .map(|(name, desc)| format!("{name} ({desc})"))
            .collect();
        let choice = prompt_select("Select target disk:", options);
        choice.split_whitespace().next().unwrap_or("").to_string()
    };
    let disk_path = format!("/dev/{disk}");
    println!("  Selected: {disk_path}");

    // Interface selection
    println!();
    println!("==> Configure network interfaces:");
    let ifaces = list_interfaces();
    if ifaces.len() < 2 {
        die(&format!(
            "Need at least 2 network interfaces (found {})",
            ifaces.len()
        ));
    }
    print_interface_table(&ifaces);

    let wan_iface = prompt_select("Select WAN interface (upstream/internet):", ifaces.clone());
    println!("  WAN: {wan_iface} -> will be renamed to 'wan'");

    let trunk_ifaces: Vec<String> = ifaces.into_iter().filter(|i| i != &wan_iface).collect();
    let trunk_iface = if trunk_ifaces.len() == 1 {
        println!(
            "  TRUNK: {} -> will be renamed to 'trunk' (only remaining interface)",
            trunk_ifaces[0]
        );
        trunk_ifaces[0].clone()
    } else {
        let choice = prompt_select("Select trunk interface (local network / switch uplink):", trunk_ifaces.clone());
        println!("  TRUNK: {choice} -> will be renamed to 'trunk'");
        choice
    };

    let wan_mac = get_mac(&wan_iface);
    let trunk_mac = get_mac(&trunk_iface);

    // Optional management interface
    let mut remaining_ifaces: Vec<String> = trunk_ifaces
        .into_iter()
        .filter(|i| i != &trunk_iface)
        .collect();
    let (mgmt_iface, mgmt_mac, subnet_mgmt) = if !remaining_ifaces.is_empty()
        && prompt_confirm("Configure a management interface? (for PVE/out-of-band access)")
    {
        let iface = if remaining_ifaces.len() == 1 {
            println!(
                "  MGMT: {} -> will be renamed to 'mgmt' (only remaining interface)",
                remaining_ifaces[0]
            );
            remaining_ifaces[0].clone()
        } else {
            let choice =
                prompt_select("Select management interface:", remaining_ifaces.clone());
            println!("  MGMT: {choice} -> will be renamed to 'mgmt'");
            choice
        };
        remaining_ifaces.retain(|i| i != &iface);
        let mac = get_mac(&iface);
        println!();
        println!("==> Configure management network:");
        let subnet = prompt_text_validated(
            "Management subnet (router IP/prefix)",
            "10.99.0.1/24",
            |v| {
                if v.parse::<IpNetwork>().is_ok() {
                    None
                } else {
                    Some("Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).")
                }
            },
        );
        println!("  Management subnet: {subnet}");
        (Some(iface), Some(mac), Some(subnet))
    } else {
        (None, None, None)
    };

    // Rename remaining interfaces
    let iface_name_re = Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{0,14}$").unwrap();
    let mut used_names: Vec<String> = vec![
        "wan".to_string(),
        "trunk".to_string(),
        "lo".to_string(),
    ];
    if mgmt_iface.is_some() {
        used_names.push("mgmt".to_string());
    }
    let mut extra_ifaces: Vec<(String, String, String)> = Vec::new();
    if !remaining_ifaces.is_empty() {
        println!();
        println!("==> Name remaining interfaces:");
        for iface in &remaining_ifaces {
            let mac = get_mac(iface);
            let driver = get_iface_driver(iface);
            let new_name = prompt_text_validated(
                &format!("Name for {iface} ({mac}, {driver})"),
                iface,
                |v| {
                    if !iface_name_re.is_match(v) {
                        return Some(
                            "Invalid name. Use 1-15 chars: letters, digits, hyphens, underscores.",
                        );
                    }
                    if used_names.iter().any(|n| n == v) {
                        return Some("Name already in use.");
                    }
                    None
                },
            );
            used_names.push(new_name.clone());
            if new_name != *iface {
                println!("  {iface} -> will be renamed to '{new_name}'");
            } else {
                println!("  {iface} -> keeping current name");
            }
            extra_ifaces.push((iface.clone(), new_name, mac));
        }
    }

    // WAN IPv6
    println!();
    let wan_enable_ipv6 = prompt_confirm("Enable IPv6 on WAN? (requires ISP support)");
    if wan_enable_ipv6 {
        println!("  WAN IPv6 enabled (will request prefix delegation from ISP).");
    }

    // VLAN configuration
    println!();
    let vlan_aware_switch =
        prompt_confirm("Do you have a VLAN-aware managed switch?");

    let vlans = if vlan_aware_switch {
        println!();
        println!("==> Configure VLANs:");
        let vlan_input = prompt_text_validated(
            "VLAN IDs (comma-separated, e.g. 10,20,30)",
            "10,20",
            |v| {
                let ids: Vec<&str> = v.split(',').map(|s| s.trim()).collect();
                for id_str in &ids {
                    match id_str.parse::<u16>() {
                        Ok(id) if id > 1 && id <= 4094 => {}
                        _ => return Some("All VLAN IDs must be numbers between 2 and 4094."),
                    }
                }
                None
            },
        );
        let vlan_ids: Vec<u16> = vlan_input
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        println!();
        let mut vlan_names: Vec<String> = used_names.clone();
        vlan_ids
            .iter()
            .map(|&id| {
                let base = format!("10.99.{}", id);
                prompt_vlan_config(id, &base, &mut vlan_names)
            })
            .collect()
    } else {
        // Simple mode: single VLAN 1
        println!();
        println!("==> Configure trunk network:");
        let subnet = prompt_text_validated("Trunk subnet (router IP/prefix)", "10.99.1.1/24", |v| {
            if v.parse::<IpNetwork>().is_ok() {
                None
            } else {
                Some("Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).")
            }
        });
        println!("  Subnet: {subnet}");

        let router_ip = subnet
            .split_once('/')
            .map(|(ip, _)| ip)
            .unwrap_or(&subnet)
            .to_string();
        let network_base = router_ip
            .rsplit_once('.')
            .map(|(base, _)| base)
            .unwrap_or(&router_ip)
            .to_string();

        println!();
        println!("==> Configure DHCP pool:");
        let enable_ipv6 = prompt_confirm("Enable IPv6 on trunk?");
        let subnet_ipv6 = if enable_ipv6 {
            let v6 = prompt_text_validated(
                "Trunk IPv6 subnet (router IP/prefix)",
                "fd00:1::1/64",
                |v| {
                    if v.parse::<IpNetwork>().is_ok() {
                        None
                    } else {
                        Some("Invalid subnet. Use CIDR notation (e.g. fd00:1::1/64).")
                    }
                },
            );
            v6
        } else {
            String::new()
        };

        println!();
        println!("==> Configure DHCP pool:");
        let dhcp_start =
            prompt_text_validated("DHCP pool start", &format!("{network_base}.100"), |_| None);
        let dhcp_end =
            prompt_text_validated("DHCP pool end", &format!("{network_base}.250"), |_| None);
        println!("  Pool: {dhcp_start} - {dhcp_end}");

        vec![VlanInstallConfig {
            id: 1,
            name: String::new(),
            subnet,
            router_ip,
            egress: true,
            enable_ipv6,
            subnet_ipv6,
            dhcp_start,
            dhcp_end,
        }]
    };

    let dns_servers =
        prompt_text_validated("DNS servers for DHCP clients", "1.1.1.1, 1.0.0.1", |_| None);
    println!("  DNS: {dns_servers}");

    InstallConfig {
        hostname,
        disk: disk_path,
        wan_iface,
        trunk_iface,
        wan_mac,
        trunk_mac,
        mgmt_iface,
        mgmt_mac,
        subnet_mgmt,
        extra_ifaces,
        wan_enable_ipv6,
        vlan_aware_switch,
        vlans,
        dns_servers,
        git_remote,
    }
}

fn show_summary(cfg: &InstallConfig) {
    println!();
    println!("==> Installation summary:");
    println!("  Hostname:     {}", cfg.hostname);
    println!("  Disk:         {}", cfg.disk);
    println!("  WAN:          {} ({}) -> wan (IPv4{}", cfg.wan_iface, cfg.wan_mac, if cfg.wan_enable_ipv6 { " + IPv6)" } else { ")" });
    println!("  TRUNK:        {} ({}) -> trunk", cfg.trunk_iface, cfg.trunk_mac);
    if let (Some(ref iface), Some(ref mac), Some(ref subnet)) =
        (&cfg.mgmt_iface, &cfg.mgmt_mac, &cfg.subnet_mgmt)
    {
        println!("  MGMT:         {iface} ({mac}) -> mgmt");
        println!("  MGMT subnet:  {subnet}");
    }
    for (orig, new_name, mac) in &cfg.extra_ifaces {
        if new_name != orig {
            println!("  EXTRA:        {orig} ({mac}) -> {new_name}");
        } else {
            println!("  EXTRA:        {orig} ({mac})");
        }
    }
    println!("  Switch mode:  {}", if cfg.vlan_aware_switch { "VLAN-aware" } else { "simple (VLAN 1)" });
    for vlan in &cfg.vlans {
        let egress_label = if vlan.egress { "yes" } else { "no" };
        let ipv6_label = if vlan.enable_ipv6 {
            format!(", IPv6: {}", vlan.subnet_ipv6)
        } else {
            String::new()
        };
        let name_label = if vlan.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", vlan.name)
        };
        println!(
            "  VLAN {:>4}{}: {} (internet: {}, DHCP: {}-{}{})",
            vlan.id, name_label, vlan.subnet, egress_label, vlan.dhcp_start, vlan.dhcp_end, ipv6_label
        );
    }
    println!("  DNS servers:  {}", cfg.dns_servers);
    if let Some(ref remote) = cfg.git_remote {
        println!("  Git remote:   {remote}");
    }
    println!();
    println!("  Save this summary for future reference (copy or screenshot).");
    println!();
    println!("  WARNING: This will ERASE ALL DATA on {}", cfg.disk);
    println!();
}

// --- Disk operations ---

fn partition_disk(disk: &str) {
    println!("==> Partitioning {disk}...");
    run_cmd("wipefs", &["-af", disk]);
    run_cmd(
        "parted",
        &[
            "-s",
            disk,
            "mklabel",
            "gpt",
            "mkpart",
            "NIFTY_BOOT",
            "fat32",
            "1MiB",
            "513MiB",
            "set",
            "1",
            "esp",
            "on",
            "mkpart",
            "NIFTY_ROOT",
            "ext4",
            "513MiB",
            "8705MiB",
            "mkpart",
            "NIFTY_VAR",
            "ext4",
            "8705MiB",
            "100%",
        ],
    );
    run_cmd("udevadm", &["settle"]);
    std::thread::sleep(std::time::Duration::from_secs(1));
}

fn partition_paths(disk: &str) -> (String, String, String) {
    let sep = if disk.contains("nvme") || disk.contains("mmcblk") {
        "p"
    } else {
        ""
    };
    (
        format!("{disk}{sep}1"),
        format!("{disk}{sep}2"),
        format!("{disk}{sep}3"),
    )
}

fn format_partitions(boot: &str, root: &str, var: &str) {
    println!("==> Formatting partitions...");
    run_cmd("mkfs.vfat", &["-F", "32", "-n", "NIFTY_BOOT", boot]);
    run_cmd("mkfs.ext4", &["-F", "-L", "NIFTY_ROOT", "-q", root]);
    run_cmd("mkfs.ext4", &["-F", "-L", "NIFTY_VAR", "-q", var]);
}

fn mount_partitions(mnt: &str, boot: &str, root: &str, var: &str) {
    println!("==> Mounting filesystems...");
    run_cmd("mount", &[root, mnt]);
    for dir in [
        "boot", "var", "run", "tmp", "home", "root", "etc", "proc", "sys", "dev",
    ] {
        fs::create_dir_all(format!("{mnt}/{dir}")).ok();
    }
    run_cmd("mount", &[boot, &format!("{mnt}/boot")]);
    run_cmd("mount", &[var, &format!("{mnt}/var")]);
}

fn copy_system_closure(mnt: &str) {
    println!("==> Copying system closure to disk...");
    let system_path = fs::read_to_string("/etc/nifty-filter/installed-system")
        .unwrap_or_default()
        .trim()
        .to_string();
    if system_path.is_empty() {
        die("Cannot read /etc/nifty-filter/installed-system");
    }
    println!("  System: {system_path}");

    fs::create_dir_all(format!("{mnt}/nix/store")).ok();

    let output = Command::new("nix-store")
        .args(["-qR", &system_path])
        .output()
        .expect("failed to run nix-store");
    for path in String::from_utf8_lossy(&output.stdout).lines() {
        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        println!("  copying {name}");
        run_cmd("cp", &["-a", path, &format!("{mnt}/nix/store/")]);
    }

    fs::create_dir_all(format!("{mnt}/nix/var/nix/db")).ok();
    // Pipe nix-store --dump-db into nix-store --load-db
    let dump = Command::new("nix-store")
        .arg("--dump-db")
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start nix-store --dump-db");
    Command::new("nix-store")
        .args(["--load-db", "--store", mnt])
        .stdin(dump.stdout.unwrap())
        .status()
        .ok();

    println!("==> Setting up system profile...");
    fs::create_dir_all(format!("{mnt}/nix/var/nix/profiles")).ok();
    std::os::unix::fs::symlink(&system_path, format!("{mnt}/nix/var/nix/profiles/system")).ok();

    println!("==> Installing bootloader...");
    Command::new("bootctl")
        .args(["install", &format!("--esp-path={mnt}/boot")])
        .stderr(Stdio::null())
        .status()
        .ok();

    let kernel = fs::read_link(format!("{system_path}/kernel"))
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let initrd = fs::read_link(format!("{system_path}/initrd"))
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    println!("  Kernel: {kernel}");
    println!("  Initrd: {initrd}");

    run_cmd("cp", &[&kernel, &format!("{mnt}/boot/kernel")]);
    run_cmd("cp", &[&initrd, &format!("{mnt}/boot/initrd")]);

    let kernel_params = fs::read_to_string(format!("{system_path}/kernel-params"))
        .unwrap_or_default()
        .trim()
        .to_string();

    fs::create_dir_all(format!("{mnt}/boot/loader")).ok();
    fs::write(
        format!("{mnt}/boot/loader/loader.conf"),
        "default nifty-filter.conf\ntimeout 3\neditor no\n",
    )
    .ok();

    fs::create_dir_all(format!("{mnt}/boot/loader/entries")).ok();
    fs::write(
        format!("{mnt}/boot/loader/entries/nifty-filter.conf"),
        format!(
            "title   nifty-filter\nlinux   /kernel\ninitrd  /initrd\noptions init={system_path}/init {kernel_params}\n"
        ),
    )
    .ok();
    fs::write(
        format!("{mnt}/boot/loader/entries/nifty-filter-maintenance.conf"),
        format!(
            "title   nifty-filter (maintenance)\nlinux   /kernel\ninitrd  /initrd\noptions init={system_path}/init {kernel_params} rw nifty.maintenance=1\n"
        ),
    )
    .ok();
    println!("  Boot entries created");
}

fn setup_var(mnt: &str, cfg: &InstallConfig) {
    println!("==> Setting up /var...");
    for dir in [
        "nifty-filter/ssh",
        "home/admin/.ssh",
        "root",
        "log/journal",
    ] {
        fs::create_dir_all(format!("{mnt}/var/{dir}")).ok();
    }

    // Interface rename rules are now generated at boot from env vars (nifty-link service).
    // No static .link files needed.

    // Build per-VLAN env content
    let mut vlan_config = String::new();
    let vlan_ids: Vec<String> = cfg.vlans.iter().map(|v| v.id.to_string()).collect();

    if cfg.vlan_aware_switch {
        vlan_config.push_str(&format!("VLAN_AWARE_SWITCH=true\nVLANS={}\n", vlan_ids.join(",")));
    } else {
        vlan_config.push_str("VLAN_AWARE_SWITCH=false\n");
    }
    vlan_config.push('\n');

    for vlan in &cfg.vlans {
        let id = vlan.id;
        let egress_v4 = if vlan.egress { "0.0.0.0/0" } else { "" };
        let egress_v6 = if vlan.egress && vlan.enable_ipv6 { "::/0" } else { "" };
        let tcp_accept = if vlan.id == 1 || vlan.egress { "22" } else { "" };
        let udp_accept = if vlan.enable_ipv6 { "67,68,546,547" } else { "67,68" };
        let name_line = if vlan.name.is_empty() {
            String::new()
        } else {
            format!("VLAN_{id}_NAME={name}\n", id = id, name = vlan.name)
        };
        vlan_config.push_str(&format!(
            "# VLAN {id}\n\
             {name_line}\
             VLAN_{id}_SUBNET_IPV4={subnet}\n\
             VLAN_{id}_SUBNET_IPV6={subnet_v6}\n\
             VLAN_{id}_EGRESS_ALLOWED_IPV4={egress_v4}\n\
             VLAN_{id}_EGRESS_ALLOWED_IPV6={egress_v6}\n\
             VLAN_{id}_ICMP_ACCEPT=echo-request,echo-reply,destination-unreachable,time-exceeded\n\
             VLAN_{id}_TCP_ACCEPT={tcp_accept}\n\
             VLAN_{id}_UDP_ACCEPT={udp_accept}\n\
             VLAN_{id}_TCP_FORWARD=\n\
             VLAN_{id}_UDP_FORWARD=\n\
             VLAN_{id}_DHCP_ENABLED=true\n\
             VLAN_{id}_DHCP_POOL_START={dhcp_start}\n\
             VLAN_{id}_DHCP_POOL_END={dhcp_end}\n\
             VLAN_{id}_DHCP_ROUTER={router_ip}\n\
             VLAN_{id}_DHCP_DNS={router_ip}\n\
             VLAN_{id}_DHCPV6_ENABLED={dhcpv6}\n\
             VLAN_{id}_DHCPV6_POOL_START=\n\
             VLAN_{id}_DHCPV6_POOL_END=\n\n",
            id = id,
            subnet = vlan.subnet,
            subnet_v6 = vlan.subnet_ipv6,
            egress_v4 = egress_v4,
            egress_v6 = egress_v6,
            tcp_accept = tcp_accept,
            udp_accept = udp_accept,
            dhcp_start = vlan.dhcp_start,
            dhcp_end = vlan.dhcp_end,
            router_ip = vlan.router_ip,
            dhcpv6 = if vlan.enable_ipv6 { "true" } else { "false" },
        ));
    }

    // Write combined config
    let env_content = format!(
        r#"# nifty-filter configuration
# Edit this file and run: nifty-config -> Apply changes
#
# This file lives on the writable /var partition.
# The rest of the system is read-only (unless booted in maintenance mode).
ENABLED=true
HOSTNAME={hostname}

# Network interfaces (MAC addresses for rename rules)
WAN_INTERFACE=wan
WAN_MAC={wan_mac}
TRUNK_INTERFACE=trunk
TRUNK_MAC={trunk_mac}
{mgmt_config}{extra_ifaces_config}
# WAN protocol enablement
WAN_ENABLE_IPV4=true
WAN_ENABLE_IPV6={wan_ipv6}

# ICMP types accepted on WAN
WAN_ICMP_ACCEPT=
WAN_ICMPV6_ACCEPT=nd-neighbor-solicit,nd-neighbor-advert,nd-router-solicit,nd-router-advert,destination-unreachable,packet-too-big,time-exceeded

# TCP/UDP ports the router accepts from WAN
WAN_TCP_ACCEPT=22
WAN_UDP_ACCEPT=

# Port forwarding rules from WAN
# Format: incoming_port:destination_ip:destination_port
WAN_TCP_FORWARD=
WAN_UDP_FORWARD=

# VLAN configuration
{vlan_config}
# DNS servers for DHCP clients (global upstream)
DHCP_UPSTREAM_DNS="{dns}"
"#,
        hostname = cfg.hostname,
        wan_mac = cfg.wan_mac,
        trunk_mac = cfg.trunk_mac,
        mgmt_config = match (&cfg.mgmt_iface, &cfg.mgmt_mac, &cfg.subnet_mgmt) {
            (Some(_), Some(mac), Some(subnet)) => format!(
                "MGMT_INTERFACE=mgmt\nMGMT_MAC={mac}\nMGMT_SUBNET={subnet}\n"
            ),
            _ => String::new(),
        },
        extra_ifaces_config = {
            let mut lines = Vec::new();
            for (_, new_name, mac) in &cfg.extra_ifaces {
                lines.push(format!("{mac}={new_name}"));
            }
            if !lines.is_empty() {
                format!("\n# Extra interface rename rules (MAC=name)\nEXTRA_LINKS=\"{}\"\n", lines.join(","))
            } else {
                String::new()
            }
        },
        wan_ipv6 = if cfg.wan_enable_ipv6 { "true" } else { "false" },
        vlan_config = vlan_config,
        dns = cfg.dns_servers,
    );
    let env_path = format!("{mnt}/var/nifty-filter/nifty-filter.env");
    fs::write(&env_path, env_content).ok();
    run_cmd("chmod", &["0600", &env_path]);

    // Copy SSH authorized keys
    println!("==> Copying SSH authorized keys...");
    let dest_ssh = format!("{mnt}/var/home/admin/.ssh");
    run_cmd("cp", &[AUTH_KEYS, &format!("{dest_ssh}/authorized_keys")]);
    run_cmd("chmod", &["0700", &dest_ssh]);
    run_cmd("chmod", &["0600", &format!("{dest_ssh}/authorized_keys")]);
    run_cmd(
        "chown",
        &["-R", "1000:100", &format!("{mnt}/var/home/admin")],
    );

    // Preserve SSH host keys
    println!("==> Preserving SSH host keys...");
    for dir in ["/var/nifty-filter/ssh", "/etc/ssh"] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("ssh_host_") {
                    run_cmd(
                        "cp",
                        &[
                            entry.path().to_str().unwrap_or(""),
                            &format!("{mnt}/var/nifty-filter/ssh/"),
                        ],
                    );
                }
            }
        }
    }
    println!("  Host fingerprint will be preserved across reboot");

    // Git repo
    println!("==> Initializing git repo in /var/nifty-filter...");
    let nf_dir = format!("{mnt}/var/nifty-filter");
    run_cmd("git", &["-C", &nf_dir, "init", "-b", "main"]);
    fs::write(format!("{nf_dir}/.gitignore"), "ssh/ssh_host_*\n").ok();
    run_cmd("git", &["-C", &nf_dir, "add", "-A"]);
    run_cmd(
        "git",
        &[
            "-C",
            &nf_dir,
            "-c",
            "user.name=nifty-filter",
            "-c",
            "user.email=nifty-filter@localhost",
            "commit",
            "-m",
            "initial configuration",
        ],
    );

    // Record build branch
    let build_branch = fs::read_to_string("/etc/nifty-filter/build-branch")
        .unwrap_or_else(|_| "master".to_string())
        .trim()
        .to_string();
    fs::write(format!("{nf_dir}/branch"), &build_branch).ok();
    println!("  Build branch: {build_branch}");

    if let Some(ref remote) = cfg.git_remote {
        run_cmd("git", &["-C", &nf_dir, "remote", "add", "origin", remote]);
        println!("==> Cloning source repo for on-device upgrades...");
        let src_dir = format!("{nf_dir}/src");
        if run_cmd("git", &["clone", "-b", &build_branch, remote, &src_dir]) {
            run_cmd("chown", &["-R", "1000:100", &src_dir]);
        } else {
            println!("  WARNING: Could not clone source repo. On-device upgrades will need manual setup.");
        }
        println!("  Git remote set: {remote}");
    }

    // Final ownership
    run_cmd("chown", &["-R", "1000:100", &nf_dir]);
}

fn unmount_and_shutdown(mnt: &str) {
    println!("==> Unmounting...");
    run_cmd("umount", &["-R", mnt]);

    println!();
    println!("Installation complete!");
    println!("Remove the installation media, then power on the system.");
    println!();
    for i in (1..=3).rev() {
        eprint!("\rShutting down in {i:2} seconds... ");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    eprintln!();
    run_cmd("systemctl", &["poweroff"]);
}

// --- PVE mode: config-only install (no disk operations) ---

fn setup_var_pve(cfg: &InstallConfig) {
    println!("==> Writing configuration to /var/nifty-filter...");

    for dir in [
        "nifty-filter/ssh",
        "home/admin/.ssh",
        "root",
        "log/journal",
    ] {
        fs::create_dir_all(format!("/var/{dir}")).ok();
    }

    // Reuse the same env file generation from setup_var
    // Build per-VLAN env content
    let mut vlan_config = String::new();
    let vlan_ids: Vec<String> = cfg.vlans.iter().map(|v| v.id.to_string()).collect();

    if cfg.vlan_aware_switch {
        vlan_config.push_str(&format!("VLAN_AWARE_SWITCH=true\nVLANS={}\n", vlan_ids.join(",")));
    } else {
        vlan_config.push_str("VLAN_AWARE_SWITCH=false\n");
    }
    vlan_config.push('\n');

    for vlan in &cfg.vlans {
        let id = vlan.id;
        let egress_v4 = if vlan.egress { "0.0.0.0/0" } else { "" };
        let egress_v6 = if vlan.egress && vlan.enable_ipv6 { "::/0" } else { "" };
        let tcp_accept = if vlan.id == 1 || vlan.egress { "22" } else { "" };
        let udp_accept = if vlan.enable_ipv6 { "67,68,546,547" } else { "67,68" };
        let name_line = if vlan.name.is_empty() {
            String::new()
        } else {
            format!("VLAN_{id}_NAME={name}\n", id = id, name = vlan.name)
        };
        vlan_config.push_str(&format!(
            "# VLAN {id}\n\
             {name_line}\
             VLAN_{id}_SUBNET_IPV4={subnet}\n\
             VLAN_{id}_SUBNET_IPV6={subnet_v6}\n\
             VLAN_{id}_EGRESS_ALLOWED_IPV4={egress_v4}\n\
             VLAN_{id}_EGRESS_ALLOWED_IPV6={egress_v6}\n\
             VLAN_{id}_ICMP_ACCEPT=echo-request,echo-reply,destination-unreachable,time-exceeded\n\
             VLAN_{id}_TCP_ACCEPT={tcp_accept}\n\
             VLAN_{id}_UDP_ACCEPT={udp_accept}\n\
             VLAN_{id}_TCP_FORWARD=\n\
             VLAN_{id}_UDP_FORWARD=\n\
             VLAN_{id}_DHCP_ENABLED=true\n\
             VLAN_{id}_DHCP_POOL_START={dhcp_start}\n\
             VLAN_{id}_DHCP_POOL_END={dhcp_end}\n\
             VLAN_{id}_DHCP_ROUTER={router_ip}\n\
             VLAN_{id}_DHCP_DNS={router_ip}\n\
             VLAN_{id}_DHCPV6_ENABLED={dhcpv6}\n\
             VLAN_{id}_DHCPV6_POOL_START=\n\
             VLAN_{id}_DHCPV6_POOL_END=\n\n",
            id = id,
            subnet = vlan.subnet,
            subnet_v6 = vlan.subnet_ipv6,
            egress_v4 = egress_v4,
            egress_v6 = egress_v6,
            tcp_accept = tcp_accept,
            udp_accept = udp_accept,
            dhcp_start = vlan.dhcp_start,
            dhcp_end = vlan.dhcp_end,
            router_ip = vlan.router_ip,
            dhcpv6 = if vlan.enable_ipv6 { "true" } else { "false" },
        ));
    }

    let env_content = format!(
        r#"# nifty-filter configuration
# Edit this file and run: nifty-config -> Apply changes
#
# This file lives on the writable /var partition.
# The rest of the system is read-only (unless booted in maintenance mode).
ENABLED=true
HOSTNAME={hostname}

# Network interfaces (MAC addresses for rename rules)
WAN_INTERFACE=wan
WAN_MAC={wan_mac}
TRUNK_INTERFACE=trunk
TRUNK_MAC={trunk_mac}
{mgmt_config}{extra_ifaces_config}
# WAN protocol enablement
WAN_ENABLE_IPV4=true
WAN_ENABLE_IPV6={wan_ipv6}

# ICMP types accepted on WAN
WAN_ICMP_ACCEPT=
WAN_ICMPV6_ACCEPT=nd-neighbor-solicit,nd-neighbor-advert,nd-router-solicit,nd-router-advert,destination-unreachable,packet-too-big,time-exceeded

# TCP/UDP ports the router accepts from WAN
WAN_TCP_ACCEPT=22
WAN_UDP_ACCEPT=

# Port forwarding rules from WAN
# Format: incoming_port:destination_ip:destination_port
WAN_TCP_FORWARD=
WAN_UDP_FORWARD=

# VLAN configuration
{vlan_config}
# DNS servers for DHCP clients (global upstream)
DHCP_UPSTREAM_DNS="{dns}"
"#,
        hostname = cfg.hostname,
        wan_mac = cfg.wan_mac,
        trunk_mac = cfg.trunk_mac,
        mgmt_config = match (&cfg.mgmt_iface, &cfg.mgmt_mac, &cfg.subnet_mgmt) {
            (Some(_), Some(mac), Some(subnet)) => format!(
                "MGMT_INTERFACE=mgmt\nMGMT_MAC={mac}\nMGMT_SUBNET={subnet}\n"
            ),
            _ => String::new(),
        },
        extra_ifaces_config = {
            let mut lines = Vec::new();
            for (_, new_name, mac) in &cfg.extra_ifaces {
                lines.push(format!("{mac}={new_name}"));
            }
            if !lines.is_empty() {
                format!("\n# Extra interface rename rules (MAC=name)\nEXTRA_LINKS=\"{}\"\n", lines.join(","))
            } else {
                String::new()
            }
        },
        wan_ipv6 = if cfg.wan_enable_ipv6 { "true" } else { "false" },
        vlan_config = vlan_config,
        dns = cfg.dns_servers,
    );
    let env_path = "/var/nifty-filter/nifty-filter.env";
    fs::write(env_path, env_content).ok();
    run_cmd("chmod", &["0600", env_path]);

    // Initialize git repo
    println!("==> Initializing git repo in /var/nifty-filter...");
    let nf_dir = "/var/nifty-filter";
    if !Path::new(&format!("{nf_dir}/.git")).exists() {
        run_cmd("git", &["-C", nf_dir, "init", "-b", "main"]);
    }
    fs::write(format!("{nf_dir}/.gitignore"), "ssh/ssh_host_*\n").ok();
    run_cmd("git", &["-C", nf_dir, "add", "-A"]);
    run_cmd(
        "git",
        &[
            "-C",
            nf_dir,
            "-c",
            "user.name=nifty-filter",
            "-c",
            "user.email=nifty-filter@localhost",
            "commit",
            "-m",
            "initial configuration",
        ],
    );

    // Record build branch (read from /etc, persist to /var)
    let build_branch = fs::read_to_string("/etc/nifty-filter/build-branch")
        .unwrap_or_else(|_| "master".to_string())
        .trim()
        .to_string();
    fs::write(format!("{nf_dir}/branch"), &build_branch).ok();
    println!("  Build branch: {build_branch}");

    if let Some(ref remote) = cfg.git_remote {
        run_cmd("git", &["-C", nf_dir, "remote", "add", "origin", remote]);
        println!("==> Cloning source repo for on-device upgrades...");
        let src_dir = format!("{nf_dir}/src");
        if run_cmd("git", &["clone", "-b", &build_branch, remote, &src_dir]) {
            run_cmd("chown", &["-R", "1000:100", &src_dir]);
        } else {
            println!("  WARNING: Could not clone source repo.");
        }
    }

    run_cmd("chown", &["-R", "1000:100", nf_dir]);
}

fn show_pve_summary(cfg: &InstallConfig) {
    println!();
    println!("==> Configuration summary:");
    println!("  Hostname:     {}", cfg.hostname);
    println!("  WAN:          {} ({}) -> wan (IPv4{}", cfg.wan_iface, cfg.wan_mac, if cfg.wan_enable_ipv6 { " + IPv6)" } else { ")" });
    println!("  TRUNK:        {} ({}) -> trunk", cfg.trunk_iface, cfg.trunk_mac);
    if let (Some(ref iface), Some(ref mac), Some(ref subnet)) =
        (&cfg.mgmt_iface, &cfg.mgmt_mac, &cfg.subnet_mgmt)
    {
        println!("  MGMT:         {iface} ({mac}) -> mgmt");
        println!("  MGMT subnet:  {subnet}");
    }
    for (orig, new_name, mac) in &cfg.extra_ifaces {
        if new_name != orig {
            println!("  EXTRA:        {orig} ({mac}) -> {new_name}");
        } else {
            println!("  EXTRA:        {orig} ({mac})");
        }
    }
    println!("  Switch mode:  {}", if cfg.vlan_aware_switch { "VLAN-aware" } else { "simple (VLAN 1)" });
    for vlan in &cfg.vlans {
        let egress_label = if vlan.egress { "yes" } else { "no" };
        let ipv6_label = if vlan.enable_ipv6 {
            format!(", IPv6: {}", vlan.subnet_ipv6)
        } else {
            String::new()
        };
        let name_label = if vlan.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", vlan.name)
        };
        println!(
            "  VLAN {:>4}{}: {} (internet: {}, DHCP: {}-{}{})",
            vlan.id, name_label, vlan.subnet, egress_label, vlan.dhcp_start, vlan.dhcp_end, ipv6_label
        );
    }
    println!("  DNS servers:  {}", cfg.dns_servers);
    if let Some(ref remote) = cfg.git_remote {
        println!("  Git remote:   {remote}");
    }
    println!();
}

// --- fw_cfg helpers (QEMU firmware config passed from PVE host) ---

fn read_fw_cfg(name: &str) -> Option<String> {
    let path = format!("/sys/firmware/qemu_fw_cfg/by_name/opt/nifty/{name}/raw");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn find_iface_by_mac(mac: &str) -> Option<String> {
    let ifaces = list_interfaces();
    for iface in &ifaces {
        if get_mac(iface).eq_ignore_ascii_case(mac) {
            return Some(iface.clone());
        }
    }
    None
}

/// Get the PCI bus address for an interface (e.g. "0000:01:00.0")
fn get_iface_pci_addr(iface: &str) -> String {
    let path = format!("/sys/class/net/{iface}/device");
    fs::read_link(&path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default()
}

/// Sort interfaces by PCI bus address (QEMU assigns slots sequentially for hostpci0, hostpci1, ...)
fn sort_ifaces_by_pci(ifaces: &[String]) -> Vec<String> {
    let mut with_addr: Vec<(String, String)> = ifaces
        .iter()
        .map(|i| (i.clone(), get_iface_pci_addr(i)))
        .collect();
    with_addr.sort_by(|a, b| a.1.cmp(&b.1));
    with_addr.into_iter().map(|(i, _)| i).collect()
}

/// PVE-specific config gathering: reads interface assignments from fw_cfg,
/// skipping the interactive interface selection prompts.
fn gather_config_pve(git_remote: Option<String>) -> InstallConfig {
    let hostname_re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap();

    // Read fw_cfg
    let mgmt_mac_cfg = read_fw_cfg("mgmt_mac");
    let nic_roles_cfg = read_fw_cfg("nic_roles"); // e.g. "wan,trunk" or "wan,trunk,extra1"
    let wan_mac_cfg = read_fw_cfg("wan_mac");   // only set for virtual NICs
    let trunk_mac_cfg = read_fw_cfg("trunk_mac"); // only set for virtual NICs

    // Show interface table for context
    println!("==> Network interfaces (auto-detected from PVE):");
    let ifaces = list_interfaces();
    print_interface_table(&ifaces);

    // Identify mgmt interface by MAC (always a virtio NIC with known MAC)
    let mgmt_iface = mgmt_mac_cfg.as_ref().and_then(|mac| find_iface_by_mac(mac));

    // Identify WAN/trunk: try MAC first (works for virtual NICs), then fall back
    // to PCI bus order (works for PCI passthrough where MAC isn't passed)
    let mut wan_iface: Option<String> = wan_mac_cfg.as_ref().and_then(|mac| find_iface_by_mac(mac));
    let mut trunk_iface: Option<String> = trunk_mac_cfg.as_ref().and_then(|mac| find_iface_by_mac(mac));

    // If MACs didn't resolve, use PCI bus order + nic_roles
    if wan_iface.is_none() || trunk_iface.is_none() {
        // Get non-mgmt interfaces sorted by PCI bus address
        let mgmt_name = mgmt_iface.as_deref().unwrap_or("");
        let non_mgmt: Vec<String> = ifaces.iter()
            .filter(|i| i.as_str() != mgmt_name)
            .cloned()
            .collect();
        let sorted = sort_ifaces_by_pci(&non_mgmt);

        // Parse roles from fw_cfg (default: "wan:trunk", colon-separated to avoid QEMU comma issues)
        let roles_str = nic_roles_cfg.as_deref().unwrap_or("wan:trunk");
        let roles: Vec<&str> = roles_str.split(':').collect();

        // Assign roles by position
        for (i, role) in roles.iter().enumerate() {
            if i >= sorted.len() {
                break;
            }
            match *role {
                "wan" if wan_iface.is_none() => wan_iface = Some(sorted[i].clone()),
                "trunk" if trunk_iface.is_none() => trunk_iface = Some(sorted[i].clone()),
                _ => {}
            }
        }
    }

    // Validate we found the required interfaces (fall back to manual if still missing)
    let wan_iface = match wan_iface {
        Some(i) => {
            let mac = get_mac(&i);
            println!("  WAN: {i} ({mac}) -> will be renamed to 'wan'");
            i
        }
        None => {
            println!("  WARNING: Could not auto-detect WAN interface.");
            let choice = prompt_select("Select WAN interface:", ifaces.clone());
            println!("  WAN: {choice} -> will be renamed to 'wan'");
            choice
        }
    };

    let trunk_iface = match trunk_iface {
        Some(i) => {
            let mac = get_mac(&i);
            println!("  TRUNK: {i} ({mac}) -> will be renamed to 'trunk'");
            i
        }
        None => {
            println!("  WARNING: Could not auto-detect trunk interface.");
            let remaining: Vec<String> = ifaces.iter().filter(|i| **i != wan_iface).cloned().collect();
            let choice = prompt_select("Select trunk interface:", remaining);
            println!("  TRUNK: {choice} -> will be renamed to 'trunk'");
            choice
        }
    };

    let wan_mac = get_mac(&wan_iface);
    let trunk_mac = get_mac(&trunk_iface);

    // Management interface (auto-detected)
    let (mgmt_iface_name, mgmt_mac_val, subnet_mgmt) = match mgmt_iface {
        Some(ref iface) => {
            let mac = get_mac(iface);
            println!("  MGMT: {iface} ({mac}) -> will be renamed to 'mgmt'");
            println!();
            println!("==> Configure management network:");
            let subnet = prompt_text_validated(
                "Management subnet (router IP/prefix)",
                "10.99.0.1/24",
                |v| {
                    if v.parse::<IpNetwork>().is_ok() {
                        None
                    } else {
                        Some("Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).")
                    }
                },
            );
            println!("  Management subnet: {subnet}");
            (Some(iface.clone()), Some(mac), Some(subnet))
        }
        None => (None, None, None),
    };

    // Remaining interfaces
    let mut used: Vec<&str> = vec![&wan_iface, &trunk_iface];
    if let Some(ref m) = mgmt_iface {
        used.push(m.as_str());
    }
    let remaining_ifaces: Vec<String> = ifaces.iter()
        .filter(|i| !used.contains(&i.as_str()))
        .cloned()
        .collect();

    let iface_name_re = Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{0,14}$").unwrap();
    let mut used_names: Vec<String> = vec![
        "wan".to_string(),
        "trunk".to_string(),
        "lo".to_string(),
    ];
    if mgmt_iface_name.is_some() {
        used_names.push("mgmt".to_string());
    }
    let mut extra_ifaces: Vec<(String, String, String)> = Vec::new();
    if !remaining_ifaces.is_empty() {
        println!();
        println!("==> Name remaining interfaces:");
        for iface in &remaining_ifaces {
            let mac = get_mac(iface);
            let driver = get_iface_driver(iface);
            let new_name = prompt_text_validated(
                &format!("Name for {iface} ({mac}, {driver})"),
                iface,
                |v| {
                    if !iface_name_re.is_match(v) {
                        return Some("Invalid name. Use 1-15 chars: letters, digits, hyphens, underscores.");
                    }
                    if used_names.iter().any(|n| n == v) {
                        return Some("Name already in use.");
                    }
                    None
                },
            );
            used_names.push(new_name.clone());
            if new_name != *iface {
                println!("  {iface} -> will be renamed to '{new_name}'");
            } else {
                println!("  {iface} -> keeping current name");
            }
            extra_ifaces.push((iface.clone(), new_name, mac));
        }
    }

    // Hostname
    println!();
    println!("==> Configure hostname:");
    let hostname = prompt_text_validated("Hostname for this router", "nifty-filter", |v| {
        if hostname_re.is_match(v) {
            None
        } else {
            Some("Invalid hostname. Must be 1-63 chars: letters, digits, hyphens.")
        }
    });
    println!("  Hostname: {hostname}");

    // WAN IPv6
    println!();
    let wan_enable_ipv6 = prompt_confirm("Enable IPv6 on WAN? (requires ISP support)");
    if wan_enable_ipv6 {
        println!("  WAN IPv6 enabled.");
    }

    // VLAN configuration
    println!();
    let vlan_aware_switch = prompt_confirm("Do you have a VLAN-aware managed switch?");

    let vlans = if vlan_aware_switch {
        println!();
        println!("==> Configure VLANs:");
        let vlan_input = prompt_text_validated(
            "VLAN IDs (comma-separated, e.g. 10,20,30)",
            "10,20",
            |v| {
                let ids: Vec<&str> = v.split(',').map(|s| s.trim()).collect();
                for id_str in &ids {
                    match id_str.parse::<u16>() {
                        Ok(id) if id > 1 && id <= 4094 => {}
                        _ => return Some("All VLAN IDs must be numbers between 2 and 4094."),
                    }
                }
                None
            },
        );
        let vlan_ids: Vec<u16> = vlan_input
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        println!();
        let mut vlan_names: Vec<String> = used_names.clone();
        vlan_ids
            .iter()
            .map(|&id| {
                let base = format!("10.99.{}", id);
                prompt_vlan_config(id, &base, &mut vlan_names)
            })
            .collect()
    } else {
        println!();
        println!("==> Configure trunk network:");
        let subnet = prompt_text_validated("Trunk subnet (router IP/prefix)", "10.99.1.1/24", |v| {
            if v.parse::<IpNetwork>().is_ok() {
                None
            } else {
                Some("Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).")
            }
        });
        println!("  Subnet: {subnet}");

        let router_ip = subnet.split_once('/').map(|(ip, _)| ip).unwrap_or(&subnet).to_string();
        let network_base = router_ip.rsplit_once('.').map(|(base, _)| base).unwrap_or(&router_ip).to_string();

        println!();
        let enable_ipv6 = prompt_confirm("Enable IPv6 on trunk?");
        let subnet_ipv6 = if enable_ipv6 {
            prompt_text_validated("Trunk IPv6 subnet (router IP/prefix)", "fd00:1::1/64", |v| {
                if v.parse::<IpNetwork>().is_ok() { None } else { Some("Invalid subnet.") }
            })
        } else {
            String::new()
        };

        println!();
        println!("==> Configure DHCP pool:");
        let dhcp_start = prompt_text_validated("DHCP pool start", &format!("{network_base}.100"), |_| None);
        let dhcp_end = prompt_text_validated("DHCP pool end", &format!("{network_base}.250"), |_| None);
        println!("  Pool: {dhcp_start} - {dhcp_end}");

        vec![VlanInstallConfig {
            id: 1,
            name: String::new(),
            subnet,
            router_ip,
            egress: true,
            enable_ipv6,
            subnet_ipv6,
            dhcp_start,
            dhcp_end,
        }]
    };

    let dns_servers = prompt_text_validated("DNS servers for DHCP clients", "1.1.1.1, 1.0.0.1", |_| None);
    println!("  DNS: {dns_servers}");

    InstallConfig {
        hostname,
        disk: String::new(), // not used in PVE mode
        wan_iface,
        trunk_iface,
        wan_mac,
        trunk_mac,
        mgmt_iface: mgmt_iface_name,
        mgmt_mac: mgmt_mac_val,
        subnet_mgmt,
        extra_ifaces,
        wan_enable_ipv6,
        vlan_aware_switch,
        vlans,
        dns_servers,
        git_remote,
    }
}

// --- Entry point ---

fn is_pve_install() -> bool {
    Path::new("/etc/nifty-filter/pve-install").exists()
}

pub fn run(git_remote: Option<String>) {
    let pve_mode = is_pve_install();

    if !pve_mode && !Path::new("/etc/nifty-filter/installed-system").exists() {
        die("This command can only be run from the nifty-filter live ISO or a PVE disk image.");
    }

    // Re-exec with sudo if not root, preserving SSH_CONNECTION
    let euid = run_cmd_output("id", &["-u"]).trim().to_string();
    if euid != "0" {
        let args: Vec<String> = std::env::args().collect();
        let mut cmd = std::process::Command::new("sudo");
        if let Ok(ssh_conn) = std::env::var("SSH_CONNECTION") {
            cmd.arg(format!("SSH_CONNECTION={ssh_conn}"));
        }
        cmd.args(&args);
        let status = cmd
            .status()
            .unwrap_or_else(|e| die(&format!("Failed to exec sudo: {}", e)));
        std::process::exit(status.code().unwrap_or(1));
    }

    println!("nifty-filter {} ({})", env!("CARGO_PKG_VERSION"), option_env!("GIT_SHA").unwrap_or("unknown"));
    println!();

    if pve_mode {
        // PVE mode: config wizard only (no disk operations)
        println!("PVE disk image detected — running configuration wizard.");
        println!("(Disk is already set up. Only writing configuration.)");
        println!();

        show_authorized_keys();

        let cfg = gather_config_pve(git_remote);
        show_pve_summary(&cfg);

        if !prompt_confirm("Apply this configuration?") {
            println!("Aborted.");
            process::exit(1);
        }
        println!();

        setup_var_pve(&cfg);

        println!();
        println!("Configuration complete!");
        println!("A reboot is required for interface renaming to take effect.");
        println!();
        for i in (1..=3).rev() {
            eprint!("\rRebooting in {i:2} seconds... ");
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        eprintln!();
        run_cmd("systemctl", &["reboot"]);
    } else {
        // ISO mode: full install (disk operations + config)
        check_authorized_keys();
        println!("==> Checking SSH authentication method...");
        check_ssh_auth();
        println!("  OK: key authentication confirmed");
        show_authorized_keys();

        let cfg = gather_config(git_remote);
        show_summary(&cfg);

        if !prompt_confirm("Proceed with installation?") {
            println!("Aborted.");
            process::exit(1);
        }
        println!();

        let mnt = run_cmd_output("mktemp", &["-d"]).trim().to_string();
        if mnt.is_empty() {
            die("Failed to create temp directory");
        }

        let (boot, root, var) = partition_paths(&cfg.disk);
        partition_disk(&cfg.disk);
        format_partitions(&boot, &root, &var);
        mount_partitions(&mnt, &boot, &root, &var);
        copy_system_closure(&mnt);
        setup_var(&mnt, &cfg);
        unmount_and_shutdown(&mnt);
    }
}
