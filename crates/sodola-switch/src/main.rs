use clap::{Parser, Subcommand};
use serde::Serialize;
use sodola_switch::{AcceptedFrameType, SodolaClient, VlanEntry, VlanPortMode};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{exit, Command};

const DEFAULT_CONFIG_DIR: &str = ".local/share/nifty-filter/sodola-switch";
const COOKIE_FILE: &str = "credentials";

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SODOLA_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG_DIR)
    }
}

fn cookie_path() -> PathBuf {
    config_dir().join(COOKIE_FILE)
}

fn save_credentials(username: &str, password: &str) -> io::Result<()> {
    let path = cookie_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{}\n{}", username, password))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn load_credentials() -> Result<(String, String), String> {
    let path = cookie_path();
    let content = fs::read_to_string(&path)
        .map_err(|_| "no saved session — run `sodola-switch login` first".to_string())?;
    let mut lines = content.lines();
    let username = lines.next()
        .ok_or_else(|| "corrupt credentials file — run `sodola-switch login`".to_string())?
        .to_string();
    let password = lines.next()
        .ok_or_else(|| "corrupt credentials file — run `sodola-switch login`".to_string())?
        .to_string();
    Ok((username, password))
}

fn remove_cookie() {
    let _ = fs::remove_file(cookie_path());
}

fn read_password() -> String {
    rpassword::prompt_password("Password: ").unwrap_or_else(|e| {
        eprintln!("Error reading password: {}", e);
        exit(1);
    })
}

#[derive(Parser)]
#[command(name = "sodola-switch")]
#[command(about = "Management tool for Sodola SL-SWTGW218AS managed switch")]
struct Cli {
    /// Switch base URL (e.g. http://192.168.2.1)
    #[arg(long, env = "SODOLA_URL", default_value = "http://192.168.2.1")]
    url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Log in and save session cookie
    Login {
        /// Admin username
        #[arg(long, default_value = "admin")]
        user: String,
        /// Admin password (prompted if not given)
        #[arg(long)]
        password: Option<String>,
    },
    /// Dump all switch state as JSON (info, stats, vlans, pvid)
    Json,
    /// Show system information (device type, MAC, IP, firmware, etc.)
    Info,
    /// Show port link status
    Status,
    /// Show port statistics (state, link, tx/rx counters)
    Stats,
    /// Show 802.1Q VLAN table
    Vlans,
    /// Create or modify an 802.1Q VLAN.
    /// Each port (1-9) is set to untagged (U), tagged (T), or not-member (X).
    /// Example: set-vlan 99 --name test --ports U,T,X,U,T,X,X,X,T
    SetVlan {
        /// VLAN ID (1-4094)
        vid: u16,
        /// VLAN name (max 16 chars)
        #[arg(long, default_value = "")]
        name: String,
        /// Port modes for ports 1-9, comma-separated: U=untagged, T=tagged, X=not-member
        #[arg(long)]
        ports: String,
    },
    /// Show per-port PVID and accepted frame type
    Pvid,
    /// Set PVID and accepted frame type for ports.
    /// Example: set-pvid --ports 1,2,3 --pvid 10 --accept untag-only
    SetPvid {
        /// Comma-separated port numbers (1-9)
        #[arg(long)]
        ports: String,
        /// PVID to assign (1-4094)
        #[arg(long)]
        pvid: u16,
        /// Accepted frame type: all, tag-only, untag-only
        #[arg(long, default_value = "all")]
        accept: String,
    },
    /// Delete one or more VLANs by ID
    DeleteVlan {
        /// VLAN IDs to delete
        vids: Vec<u16>,
    },
    /// Factory reset the switch (restores all defaults)
    FactoryReset,
    /// Reboot the switch
    Reboot,
    /// Save running configuration to flash ROM
    Save,
    /// Download switch configuration backup
    Backup {
        /// Output file path
        #[arg(short, long, default_value = "switch_cfg.bin")]
        output: PathBuf,
    },
    /// Restore a configuration backup from file (reboot required to take effect)
    Restore {
        /// Input file path
        #[arg(short, long, default_value = "switch_cfg.bin")]
        input: PathBuf,
    },
    /// Add IP address to trunk interface so the switch is reachable (requires sudo)
    RouteUp {
        /// Network interface connected to the switch
        #[arg(long, env = "SODOLA_MGMT_IFACE", default_value = "trunk")]
        iface: String,
        /// IP address to assign (in the switch's subnet)
        #[arg(long, env = "SODOLA_ROUTER_IP", default_value = "192.168.2.2/24")]
        ip: String,
    },
    /// Remove IP address from trunk interface (requires sudo)
    RouteDown {
        /// Network interface connected to the switch
        #[arg(long, env = "SODOLA_MGMT_IFACE", default_value = "trunk")]
        iface: String,
        /// IP address to remove
        #[arg(long, env = "SODOLA_ROUTER_IP", default_value = "192.168.2.2/24")]
        ip: String,
    },
    /// Log out and remove saved session
    Logout,
    /// Supervise switch state: compare against desired config and fix discrepancies
    Supervise {
        /// Path to the config env file (default: $SODOLA_CONFIG_DIR/config.env)
        #[arg(long, env = "SODOLA_SWITCH_CONFIG")]
        env_file: Option<PathBuf>,
        /// Write switch state JSON to this file after each run
        #[arg(long, env = "SODOLA_STATE_FILE")]
        state_file: Option<PathBuf>,
        /// Report discrepancies but do not apply fixes
        #[arg(long)]
        dry_run: bool,
        /// Save configuration to flash ROM after applying changes
        #[arg(long)]
        save: bool,
        /// Run as a daemon, checking every N seconds (0 = run once and exit)
        #[arg(long, env = "SODOLA_INTERVAL", default_value = "0")]
        interval: u64,
        /// Network interface for route-up (only used with --interval)
        #[arg(long, env = "SODOLA_MGMT_IFACE", default_value = "trunk")]
        iface: Option<String>,
        /// IP address for route-up (only used with --interval)
        #[arg(long, env = "SODOLA_ROUTER_IP", default_value = "192.168.2.2/24")]
        ip: Option<String>,
    },
}

fn parse_port_modes(s: &str) -> Result<[VlanPortMode; 9], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 9 {
        return Err(format!("expected 9 port modes, got {}", parts.len()));
    }
    let mut modes = [VlanPortMode::NotMember; 9];
    for (i, part) in parts.iter().enumerate() {
        modes[i] = match part.trim().to_uppercase().as_str() {
            "U" | "UNTAGGED" => VlanPortMode::Untagged,
            "T" | "TAGGED" => VlanPortMode::Tagged,
            "X" | "N" | "NONE" | "NOT-MEMBER" => VlanPortMode::NotMember,
            other => return Err(format!("invalid port mode '{}' for port {} (use U/T/X)", other, i + 1)),
        };
    }
    Ok(modes)
}

// --- Supervise support ---

struct DesiredVlan {
    vid: u16,
    name: String,
    ports: [VlanPortMode; 9],
}

struct DesiredPort {
    port: u8,
    pvid: u16,
    accepted_frame_type: AcceptedFrameType,
}

struct DesiredState {
    vlans: Vec<DesiredVlan>,
    managed_vids: HashSet<u16>,
    ports: Vec<DesiredPort>,
}

/// Parse a port-range string like "5-7,9" or "-" into a set of port numbers.
fn parse_port_range(s: &str) -> HashSet<u8> {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return HashSet::new();
    }
    let mut result = HashSet::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (start.trim().parse::<u8>(), end.trim().parse::<u8>()) {
                for p in a..=b {
                    result.insert(p);
                }
            }
        } else if let Ok(p) = part.parse::<u8>() {
            result.insert(p);
        }
    }
    result
}

/// Convert a VlanEntry (string-based port ranges) to a [VlanPortMode; 9] array.
fn vlan_entry_to_port_modes(entry: &VlanEntry) -> [VlanPortMode; 9] {
    let tagged = parse_port_range(&entry.tagged_ports);
    let untagged = parse_port_range(&entry.untagged_ports);
    let mut modes = [VlanPortMode::NotMember; 9];
    for i in 0..9 {
        let port = (i + 1) as u8;
        if tagged.contains(&port) {
            modes[i] = VlanPortMode::Tagged;
        } else if untagged.contains(&port) {
            modes[i] = VlanPortMode::Untagged;
        }
    }
    modes
}

fn port_mode_label(m: VlanPortMode) -> &'static str {
    match m {
        VlanPortMode::Untagged => "Untagged",
        VlanPortMode::Tagged => "Tagged",
        VlanPortMode::NotMember => "NotMember",
    }
}

fn parse_accepted_frame_type(s: &str) -> Result<AcceptedFrameType, String> {
    match s.to_lowercase().as_str() {
        "all" => Ok(AcceptedFrameType::All),
        "tag-only" | "tag" | "tagged" => Ok(AcceptedFrameType::TagOnly),
        "untag-only" | "untag" | "untagged" => Ok(AcceptedFrameType::UntagOnly),
        other => Err(format!("invalid frame type '{}' (use all/tag-only/untag-only)", other)),
    }
}

fn parse_desired_state(env_file: &std::path::Path) -> Result<Option<DesiredState>, String> {
    dotenvy::from_filename(env_file)
        .map_err(|e| format!("failed to load {}: {}", env_file.display(), e))?;

    // SWITCH_VLANS takes priority; fall back to VLANS (auto-adding VLAN 1)
    let vlans_str = match env::var("SWITCH_VLANS").ok().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => match env::var("VLANS").ok().filter(|s| !s.is_empty()) {
            Some(s) => format!("1,{}", s),
            None => return Ok(None),
        },
    };
    let managed_vids: HashSet<u16> = vlans_str
        .split(',')
        .map(|s| s.trim().parse::<u16>().map_err(|e| format!("bad VLAN ID '{}': {}", s.trim(), e)))
        .collect::<Result<_, _>>()?;

    if managed_vids.is_empty() {
        return Ok(None);
    }

    let mut vlans = Vec::new();
    for &vid in &managed_vids {
        let ports_key = format!("SWITCH_VLAN_{}_PORTS", vid);
        let ports_str = env::var(&ports_key)
            .map_err(|_| format!("{} not set (required for VLAN {})", ports_key, vid))?;
        let ports = parse_port_modes(&ports_str)
            .map_err(|e| format!("{}: {}", ports_key, e))?;
        // SWITCH_VLAN_{ID}_NAME takes priority, falls back to VLAN_{ID}_NAME
        let name = env::var(format!("SWITCH_VLAN_{}_NAME", vid))
            .or_else(|_| env::var(format!("VLAN_{}_NAME", vid)))
            .unwrap_or_default();
        vlans.push(DesiredVlan { vid, name, ports });
    }

    let mut ports = Vec::new();
    if let Ok(pvid_str) = env::var("SWITCH_PVID") {
        let accept_map: HashMap<u8, AcceptedFrameType> = if let Ok(accept_str) = env::var("SWITCH_ACCEPT") {
            accept_str.split(',')
                .map(|pair| {
                    let (p, t) = pair.trim().split_once(':')
                        .ok_or_else(|| format!("bad SWITCH_ACCEPT entry '{}'", pair.trim()))?;
                    let port: u8 = p.trim().parse().map_err(|e| format!("bad port '{}': {}", p.trim(), e))?;
                    let ft = parse_accepted_frame_type(t.trim())?;
                    Ok((port, ft))
                })
                .collect::<Result<_, String>>()?
        } else {
            HashMap::new()
        };

        for pair in pvid_str.split(',') {
            let (p, v) = pair.trim().split_once(':')
                .ok_or_else(|| format!("bad SWITCH_PVID entry '{}'", pair.trim()))?;
            let port: u8 = p.trim().parse().map_err(|e| format!("bad port '{}': {}", p.trim(), e))?;
            let pvid: u16 = v.trim().parse().map_err(|e| format!("bad PVID '{}': {}", v.trim(), e))?;
            if !(1..=9).contains(&port) {
                return Err(format!("port {} out of range (1-9)", port));
            }
            let accepted_frame_type = accept_map.get(&port).copied().unwrap_or(AcceptedFrameType::All);
            ports.push(DesiredPort { port, pvid, accepted_frame_type });
        }
    }

    Ok(Some(DesiredState { vlans, managed_vids, ports }))
}

fn dump_state(client: &SodolaClient, state_file: &std::path::Path) {
    #[derive(Serialize)]
    struct SwitchDump {
        timestamp: u64,
        info: sodola_switch::SwitchInfo,
        stats: Vec<sodola_switch::PortStats>,
        vlans: Vec<sodola_switch::VlanEntry>,
        pvid: Vec<sodola_switch::PortVlanSetting>,
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let info = match client.info() {
        Ok(v) => v,
        Err(e) => { eprintln!("supervise: warning: failed to dump switch state: info: {}", e); return; }
    };
    let stats = match client.port_stats() {
        Ok(v) => v,
        Err(e) => { eprintln!("supervise: warning: failed to dump switch state: stats: {}", e); return; }
    };
    let vlans = match client.vlans() {
        Ok(v) => v,
        Err(e) => { eprintln!("supervise: warning: failed to dump switch state: vlans: {}", e); return; }
    };
    let pvid = match client.pvid() {
        Ok(v) => v,
        Err(e) => { eprintln!("supervise: warning: failed to dump switch state: pvid: {}", e); return; }
    };
    let dump = SwitchDump { timestamp, info, stats, vlans, pvid };
    let json = serde_json::to_string_pretty(&dump).unwrap();
    let tmp = state_file.with_extension("json.tmp");
    if let Err(e) = fs::write(&tmp, &json).and_then(|_| fs::rename(&tmp, state_file)) {
        eprintln!("supervise: warning: failed to write {}: {}", state_file.display(), e);
    }
}

fn run_supervise(client: &mut SodolaClient, env_file: &std::path::Path, state_file: Option<&std::path::Path>, dry_run: bool, do_save: bool) {
    let desired = match parse_desired_state(env_file) {
        Ok(Some(d)) => d,
        Ok(None) => {
            eprintln!("supervise: SWITCH_VLANS not configured, skipping");
            return;
        }
        Err(e) => {
            eprintln!("supervise: config error: {}", e);
            return;
        }
    };

    let current_vlans = match client.vlans() {
        Ok(v) => v,
        Err(e) => { eprintln!("supervise: failed to read VLANs: {}", e); return; }
    };
    let current_pvid = match client.pvid() {
        Ok(p) => p,
        Err(e) => { eprintln!("supervise: failed to read PVID: {}", e); return; }
    };

    let mut changes = 0u32;

    // 1. Delete stale VLANs (not in config, not VLAN 1)
    let current_vids: HashSet<u16> = current_vlans.iter().map(|v| v.vid).collect();
    let stale: Vec<u16> = current_vids.iter()
        .filter(|&&vid| vid != 1 && !desired.managed_vids.contains(&vid))
        .copied()
        .collect();
    if !stale.is_empty() {
        for &vid in &stale {
            eprintln!("supervise: VLAN {} on switch but not in config — deleting", vid);
        }
        if !dry_run {
            if let Err(e) = client.delete_vlans(&stale) {
                eprintln!("supervise: failed to delete VLANs {:?}: {}", stale, e);
            } else {
                eprintln!("supervise: deleted VLAN(s): {:?}", stale);
            }
        }
        changes += stale.len() as u32;
    }

    // 2. Create/update VLANs
    let current_map: HashMap<u16, &VlanEntry> = current_vlans.iter().map(|v| (v.vid, v)).collect();
    for dv in &desired.vlans {
        let mut needs_update = false;
        if let Some(cur) = current_map.get(&dv.vid) {
            let cur_modes = vlan_entry_to_port_modes(cur);
            if dv.vid != 1 && cur.name != dv.name {
                eprintln!("supervise: VLAN {} name mismatch: switch={:?} config={:?} — updating",
                    dv.vid, cur.name, dv.name);
                needs_update = true;
            }
            for i in 0..9 {
                if cur_modes[i] != dv.ports[i] {
                    eprintln!("supervise: VLAN {} port {} mismatch: switch={} config={} — updating",
                        dv.vid, i + 1, port_mode_label(cur_modes[i]), port_mode_label(dv.ports[i]));
                    needs_update = true;
                }
            }
        } else {
            eprintln!("supervise: VLAN {} missing on switch — creating", dv.vid);
            needs_update = true;
        }

        if needs_update {
            if !dry_run {
                if let Err(e) = client.set_vlan(dv.vid, &dv.name, &dv.ports) {
                    eprintln!("supervise: failed to set VLAN {}: {}", dv.vid, e);
                }
            }
            changes += 1;
        }
    }

    // 3. Set PVIDs
    let pvid_map: HashMap<u8, &sodola_switch::PortVlanSetting> =
        current_pvid.iter().map(|p| (p.port, p)).collect();
    for dp in &desired.ports {
        let mut needs_update = false;
        if let Some(cur) = pvid_map.get(&dp.port) {
            if cur.pvid != dp.pvid {
                eprintln!("supervise: port {} PVID mismatch: switch={} config={} — updating",
                    dp.port, cur.pvid, dp.pvid);
                needs_update = true;
            }
            if cur.accepted_frame_type != dp.accepted_frame_type {
                eprintln!("supervise: port {} accept mismatch: switch={} config={} — updating",
                    dp.port, cur.accepted_frame_type, dp.accepted_frame_type);
                needs_update = true;
            }
        } else {
            eprintln!("supervise: port {} not found in switch PVID table — setting", dp.port);
            needs_update = true;
        }

        if needs_update && !dry_run {
            if let Err(e) = client.set_pvid(&[dp.port], dp.pvid, dp.accepted_frame_type) {
                eprintln!("supervise: failed to set PVID for port {}: {}", dp.port, e);
            }
            changes += 1;
        }
    }

    if changes == 0 {
        eprintln!("supervise: switch state matches desired config ({} VLANs, {} ports checked)",
            desired.vlans.len(), desired.ports.len());
    } else if dry_run {
        eprintln!("supervise: dry-run: {} change(s) needed (use without --dry-run to apply)", changes);
    } else {
        if do_save {
            if let Err(e) = client.save() {
                eprintln!("supervise: failed to save to flash: {}", e);
            } else {
                eprintln!("supervise: saved configuration to flash");
            }
        }
        eprintln!("supervise: applied {} change(s)", changes);
    }

    if let Some(path) = state_file {
        dump_state(client, path);
    }
}

fn route_up(iface: &str, ip: &str) {
    let output = if unsafe { libc::geteuid() } == 0 {
        Command::new("ip").args(["addr", "add", ip, "dev", iface]).output()
    } else {
        Command::new("sudo").args(["ip", "addr", "add", ip, "dev", iface]).output()
    };
    match output {
        Ok(o) if o.status.success() => {
            eprintln!("Added {} to {}", ip, iface);
        }
        Ok(o) => {
            // exit code 2 = already exists, silently ignore
            if o.status.code() != Some(2) {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("Failed to add {} to {} (exit {}): {}", ip, iface, o.status, stderr.trim());
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to run ip: {}", e);
            exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let mut client = SodolaClient::new(&cli.url);

    // Route commands run ip(8) directly — no switch connection needed
    match &cli.command {
        Commands::RouteUp { iface, ip } => {
            route_up(iface, ip);
            return;
        }
        Commands::RouteDown { iface, ip } => {
            let output = if unsafe { libc::geteuid() } == 0 {
                Command::new("ip").args(["addr", "del", ip, "dev", iface]).output()
            } else {
                Command::new("sudo").args(["ip", "addr", "del", ip, "dev", iface]).output()
            };
            match output {
                Ok(o) if o.status.success() => {
                    eprintln!("Removed {} from {}", ip, iface);
                    return;
                }
                Ok(_) => {
                    return;
                }
                Err(e) => {
                    eprintln!("Failed to run ip: {}", e);
                    exit(1);
                }
            }
        }
        _ => {}
    }

    // Login command handles its own auth flow
    if let Commands::Login { ref user, ref password } = cli.command {
        let pw = password.clone().unwrap_or_else(read_password);
        if let Err(e) = client.login(user, &pw) {
            eprintln!("Login failed: {}", e);
            exit(1);
        }
        if let Err(e) = save_credentials(user, &pw) {
            eprintln!("Warning: could not save credentials: {}", e);
        }
        eprintln!("Logged in. Credentials saved to {}", cookie_path().display());
        return;
    }

    // Supervise handles its own auth from the env file
    if let Commands::Supervise { ref env_file, ref state_file, dry_run, save, interval, ref iface, ref ip } = cli.command {
        let path = env_file.clone().unwrap_or_else(|| config_dir().join("config.env"));
        // Load env file first so SODOLA_URL/USER/PASS are available
        if let Err(e) = dotenvy::from_filename(&path) {
            eprintln!("supervise: failed to load {}: {}", path.display(), e);
            exit(1);
        }
        let url = env::var("SODOLA_URL").unwrap_or_else(|_| "http://192.168.2.1".to_string());
        let user = env::var("SODOLA_USER").unwrap_or_else(|_| "admin".to_string());
        let pass = env::var("SODOLA_PASS").unwrap_or_else(|_| "admin".to_string());

        if interval == 0 {
            // One-shot mode
            let mut client = SodolaClient::new(&url);
            if let Err(e) = client.login(&user, &pass) {
                eprintln!("supervise: login failed: {}", e);
                exit(1);
            }
            run_supervise(&mut client, &path, state_file.as_deref(), dry_run, save);
        } else {
            // Daemon mode: route-up first, then loop
            if let (Some(iface), Some(ip)) = (iface, ip) {
                route_up(iface, ip);
            }
            eprintln!("supervise: starting daemon (interval={}s)", interval);
            loop {
                let mut client = SodolaClient::new(&url);
                if let Err(e) = client.login(&user, &pass) {
                    eprintln!("supervise: login failed (will retry): {}", e);
                } else {
                    run_supervise(&mut client, &path, state_file.as_deref(), dry_run, save);
                }
                std::thread::sleep(std::time::Duration::from_secs(interval));
            }
        }
        return;
    }

    // All other commands load saved credentials and login
    match load_credentials() {
        Ok((user, password)) => {
            if let Err(e) = client.login(&user, &password) {
                eprintln!("Error: {}", e);
                exit(1);
            }
        }
        Err(msg) => {
            eprintln!("Error: {}", msg);
            exit(1);
        }
    }

    let result = match &cli.command {
        Commands::Login { .. } => unreachable!(),
        Commands::Json => {
            #[derive(Serialize)]
            struct SwitchDump {
                info: sodola_switch::SwitchInfo,
                stats: Vec<sodola_switch::PortStats>,
                vlans: Vec<sodola_switch::VlanEntry>,
                pvid: Vec<sodola_switch::PortVlanSetting>,
            }
            let info = client.info();
            let stats = client.port_stats();
            let vlans = client.vlans();
            let pvid = client.pvid();
            match (info, stats, vlans, pvid) {
                (Ok(info), Ok(stats), Ok(vlans), Ok(pvid)) => {
                    let dump = SwitchDump { info, stats, vlans, pvid };
                    println!("{}", serde_json::to_string_pretty(&dump).unwrap());
                    Ok(())
                }
                (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => Err(e),
            }
        }
        Commands::Info => {
            client.info().map(|info| println!("{}", info))
        }
        Commands::Status => {
            client.port_status().map(|ports| {
                for port in &ports {
                    println!("{}", port);
                }
            })
        }
        Commands::Stats => {
            client.port_stats().map(|stats| {
                for s in &stats {
                    println!("{}", s);
                }
            })
        }
        Commands::Vlans => {
            client.vlans().map(|vlans| {
                for vlan in &vlans {
                    println!("{}", vlan);
                }
            })
        }
        Commands::Pvid => {
            client.pvid().map(|settings| {
                for s in &settings {
                    println!("{}", s);
                }
            })
        }
        Commands::SetPvid { ports, pvid, accept } => {
            let port_nums: Result<Vec<u8>, _> = ports.split(',').map(|s| s.trim().parse::<u8>()).collect();
            let port_nums = match port_nums {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error: invalid port number: {}", e);
                    exit(1);
                }
            };
            let frame_type = match accept.to_lowercase().as_str() {
                "all" => AcceptedFrameType::All,
                "tag-only" | "tag" | "tagged" => AcceptedFrameType::TagOnly,
                "untag-only" | "untag" | "untagged" => AcceptedFrameType::UntagOnly,
                other => {
                    eprintln!("Error: invalid frame type '{}' (use all/tag-only/untag-only)", other);
                    exit(1);
                }
            };
            client.set_pvid(&port_nums, *pvid, frame_type).map(|_| {
                eprintln!("PVID set to {} for port(s) {:?}", pvid, port_nums);
            })
        }
        Commands::SetVlan { vid, name, ports } => {
            match parse_port_modes(ports) {
                Ok(modes) => {
                    client.set_vlan(*vid, name, &modes).map(|_| {
                        eprintln!("VLAN {} set successfully", vid);
                    })
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
            }
        }
        Commands::DeleteVlan { vids } => {
            client.delete_vlans(vids).map(|_| {
                eprintln!("Deleted VLAN(s): {:?}", vids);
            })
        }
        Commands::FactoryReset => {
            client.factory_reset().map(|_| eprintln!("Factory reset initiated. Switch is rebooting (~15 seconds)."))
        }
        Commands::Reboot => {
            client.reboot().map(|_| eprintln!("Switch is rebooting (~15 seconds)."))
        }
        Commands::Save => {
            client.save().map(|_| eprintln!("Configuration saved to flash."))
        }
        Commands::Backup { output } => {
            client.backup_to_file(output).map(|size| {
                eprintln!("Backup saved to {} ({} bytes)", output.display(), size);
            })
        }
        Commands::Restore { input } => {
            client.restore_from_file(input).map(|_| {
                eprintln!("Configuration restored from {}. Reboot to apply.", input.display());
            })
        }
        Commands::Logout => {
            let _ = client.logout();
            remove_cookie();
            eprintln!("Logged out. Session removed.");
            return;
        }
        Commands::Supervise { .. } | Commands::RouteUp { .. } | Commands::RouteDown { .. } => unreachable!(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        exit(1);
    }
}
