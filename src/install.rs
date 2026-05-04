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

struct InstallConfig {
    hostname: String,
    disk: String,
    wan_iface: String,
    lan_iface: String,
    wan_mac: String,
    lan_mac: String,
    mgmt_iface: Option<String>,
    mgmt_mac: Option<String>,
    subnet_mgmt: Option<String>,
    extra_ifaces: Vec<(String, String, String)>, // (original, new_name, mac)
    subnet_lan: String,
    router_ip: String,
    dhcp_start: String,
    dhcp_end: String,
    dns_servers: String,
    git_remote: Option<String>,
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

    let lan_ifaces: Vec<String> = ifaces.into_iter().filter(|i| i != &wan_iface).collect();
    let lan_iface = if lan_ifaces.len() == 1 {
        println!(
            "  LAN: {} -> will be renamed to 'lan' (only remaining interface)",
            lan_ifaces[0]
        );
        lan_ifaces[0].clone()
    } else {
        let choice = prompt_select("Select LAN interface (local network):", lan_ifaces.clone());
        println!("  LAN: {choice} -> will be renamed to 'lan'");
        choice
    };

    let wan_mac = get_mac(&wan_iface);
    let lan_mac = get_mac(&lan_iface);

    // Optional management interface
    let mut remaining_ifaces: Vec<String> = lan_ifaces
        .into_iter()
        .filter(|i| i != &lan_iface)
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
        "lan".to_string(),
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

    // LAN subnet
    println!();
    println!("==> Configure LAN network:");
    let subnet_lan = prompt_text_validated("LAN subnet (router IP/prefix)", "10.99.1.1/24", |v| {
        if v.parse::<IpNetwork>().is_ok() {
            None
        } else {
            Some("Invalid subnet. Use CIDR notation (e.g. 10.99.1.1/24).")
        }
    });
    println!("  Subnet: {subnet_lan}");

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

    // DHCP pool
    println!();
    println!("==> Configure DHCP pool:");
    let dhcp_start = prompt_text_validated("DHCP pool start", &default_start, |_| None);
    let dhcp_end = prompt_text_validated("DHCP pool end", &default_end, |_| None);
    println!("  Pool: {dhcp_start} - {dhcp_end}");

    let dns_servers =
        prompt_text_validated("DNS servers for DHCP clients", "1.1.1.1, 1.0.0.1", |_| None);
    println!("  DNS: {dns_servers}");

    InstallConfig {
        hostname,
        disk: disk_path,
        wan_iface,
        lan_iface,
        wan_mac,
        lan_mac,
        mgmt_iface,
        mgmt_mac,
        subnet_mgmt,
        extra_ifaces,
        subnet_lan,
        router_ip,
        dhcp_start,
        dhcp_end,
        dns_servers,
        git_remote,
    }
}

fn show_summary(cfg: &InstallConfig) {
    println!();
    println!("==> Installation summary:");
    println!("  Hostname:     {}", cfg.hostname);
    println!("  Disk:         {}", cfg.disk);
    println!("  WAN:          {} ({}) -> wan", cfg.wan_iface, cfg.wan_mac);
    println!("  LAN:          {} ({}) -> lan", cfg.lan_iface, cfg.lan_mac);
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
    println!("  LAN subnet:   {}", cfg.subnet_lan);
    println!("  DHCP pool:    {} - {}", cfg.dhcp_start, cfg.dhcp_end);
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
        "nifty-filter/network",
        "home/admin/.ssh",
        "root",
        "log/journal",
    ] {
        fs::create_dir_all(format!("{mnt}/var/{dir}")).ok();
    }

    // Interface rename rules
    println!("==> Creating interface rename rules...");
    fs::write(
        format!("{mnt}/var/nifty-filter/network/10-wan.link"),
        format!("[Match]\nMACAddress={}\n\n[Link]\nName=wan\n", cfg.wan_mac),
    )
    .ok();
    fs::write(
        format!("{mnt}/var/nifty-filter/network/10-lan.link"),
        format!("[Match]\nMACAddress={}\n\n[Link]\nName=lan\n", cfg.lan_mac),
    )
    .ok();
    if let Some(ref mac) = cfg.mgmt_mac {
        fs::write(
            format!("{mnt}/var/nifty-filter/network/10-mgmt.link"),
            format!("[Match]\nMACAddress={mac}\n\n[Link]\nName=mgmt\n"),
        )
        .ok();
    }
    for (orig, new_name, mac) in &cfg.extra_ifaces {
        if new_name != orig {
            fs::write(
                format!("{mnt}/var/nifty-filter/network/10-{new_name}.link"),
                format!("[Match]\nMACAddress={mac}\n\n[Link]\nName={new_name}\n"),
            )
            .ok();
        }
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

# Network interfaces
INTERFACE_LAN=lan
INTERFACE_WAN=wan
{mgmt_config}
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
DHCP_DNS="{dns}"

# DHCPv6 (enable and configure via nifty-config after install)
DHCPV6_ENABLED=false
DHCPV6_POOL_START=
DHCPV6_POOL_END=
"#,
        hostname = cfg.hostname,
        mgmt_config = match &cfg.subnet_mgmt {
            Some(subnet) => format!(
                "\n# Management interface (out-of-band access)\nINTERFACE_MGMT=mgmt\nSUBNET_MGMT={subnet}\n"
            ),
            None => String::new(),
        },
        subnet = cfg.subnet_lan,
        dhcp_start = cfg.dhcp_start,
        dhcp_end = cfg.dhcp_end,
        router_ip = cfg.router_ip,
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

// --- Entry point ---

pub fn run(git_remote: Option<String>) {
    // Only run on the live ISO
    if !Path::new("/etc/nifty-filter/installed-system").exists() {
        die("This command can only be run from the nifty-filter live ISO.");
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

    // Pre-flight
    println!("nifty-filter {} ({})", env!("CARGO_PKG_VERSION"), option_env!("GIT_SHA").unwrap_or("unknown"));
    println!();
    check_authorized_keys();
    println!("==> Checking SSH authentication method...");
    check_ssh_auth();
    println!("  OK: key authentication confirmed");
    show_authorized_keys();

    // Interactive configuration
    let cfg = gather_config(git_remote);
    show_summary(&cfg);

    if !prompt_confirm("Proceed with installation?") {
        println!("Aborted.");
        process::exit(1);
    }
    println!();

    // Create mount point
    let mnt = run_cmd_output("mktemp", &["-d"]).trim().to_string();
    if mnt.is_empty() {
        die("Failed to create temp directory");
    }

    // Install
    let (boot, root, var) = partition_paths(&cfg.disk);
    partition_disk(&cfg.disk);
    format_partitions(&boot, &root, &var);
    mount_partitions(&mnt, &boot, &root, &var);
    copy_system_closure(&mnt);
    setup_var(&mnt, &cfg);
    unmount_and_shutdown(&mnt);
}
