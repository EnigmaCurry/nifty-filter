use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{self, Command, Stdio};

use inquire::{Confirm, InquireError, Select, Text};
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
    match Confirm::new(message).with_default(false).prompt() {
        Ok(v) => v,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => false,
        Err(_) => false,
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

// --- Interactive prompts ---

struct InstallConfig {
    hostname: String,
    disk: String,
    wan_iface: String,
    lan_iface: String,
    wan_mac: String,
    lan_mac: String,
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
        choice
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
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

    let wan_iface = prompt_select(
        "Select WAN interface (upstream/internet):",
        ifaces.clone(),
    );
    println!("  WAN: {wan_iface} -> will be renamed to 'wan'");

    let lan_ifaces: Vec<String> = ifaces
        .into_iter()
        .filter(|i| i != &wan_iface)
        .collect();
    let lan_iface = if lan_ifaces.len() == 1 {
        println!(
            "  LAN: {} -> will be renamed to 'lan' (only remaining interface)",
            lan_ifaces[0]
        );
        lan_ifaces[0].clone()
    } else {
        let choice = prompt_select("Select LAN interface (local network):", lan_ifaces);
        println!("  LAN: {choice} -> will be renamed to 'lan'");
        choice
    };

    let wan_mac = get_mac(&wan_iface);
    let lan_mac = get_mac(&lan_iface);

    // LAN subnet
    println!();
    println!("==> Configure LAN network:");
    let subnet_lan = prompt_text_validated("LAN subnet (router IP/prefix)", "10.99.0.1/24", |v| {
        if v.parse::<IpNetwork>().is_ok() {
            None
        } else {
            Some("Invalid subnet. Use CIDR notation (e.g. 10.99.0.1/24).")
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
    println!(
        "  WAN:          {} ({}) -> wan",
        cfg.wan_iface, cfg.wan_mac
    );
    println!(
        "  LAN:          {} ({}) -> lan",
        cfg.lan_iface, cfg.lan_mac
    );
    println!("  LAN subnet:   {}", cfg.subnet_lan);
    println!("  DHCP pool:    {} - {}", cfg.dhcp_start, cfg.dhcp_end);
    println!("  DNS servers:  {}", cfg.dns_servers);
    if let Some(ref remote) = cfg.git_remote {
        println!("  Git remote:   {remote}");
    }
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
            "-s", disk, "mklabel", "gpt",
            "mkpart", "NIFTY_BOOT", "fat32", "1MiB", "513MiB",
            "set", "1", "esp", "on",
            "mkpart", "NIFTY_ROOT", "ext4", "513MiB", "8705MiB",
            "mkpart", "NIFTY_VAR", "ext4", "8705MiB", "100%",
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
    fs::create_dir_all(format!("{mnt}/boot")).ok();
    fs::create_dir_all(format!("{mnt}/var")).ok();
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
    run_cmd("bootctl", &["install", &format!("--esp-path={mnt}/boot")]);

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
        format!(
            "[Match]\nMACAddress={}\n\n[Link]\nName=wan\n",
            cfg.wan_mac
        ),
    )
    .ok();
    fs::write(
        format!("{mnt}/var/nifty-filter/network/10-lan.link"),
        format!(
            "[Match]\nMACAddress={}\n\n[Link]\nName=lan\n",
            cfg.lan_mac
        ),
    )
    .ok();

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
        hostname = cfg.hostname,
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
    run_cmd("chown", &["-R", "1000:100", &format!("{mnt}/var/home/admin")]);

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
        run_cmd(
            "git",
            &["-C", &nf_dir, "remote", "add", "origin", remote],
        );
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

    println!("==> Ejecting installation media...");
    run_cmd("eject", &["/dev/sr0"]);

    println!();
    println!("Installation complete!");
    println!("Remove the installation media, then power on the system.");
    println!();
    for i in (1..=10).rev() {
        eprint!("\rShutting down in {i:2} seconds... ");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    eprintln!();
    run_cmd("systemctl", &["poweroff"]);
}

// --- Entry point ---

pub fn run(git_remote: Option<String>) {
    // Must be root
    let euid = run_cmd_output("id", &["-u"]).trim().to_string();
    if euid != "0" {
        die("Must be run as root. Try: sudo nifty-filter install");
    }

    // Pre-flight
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
