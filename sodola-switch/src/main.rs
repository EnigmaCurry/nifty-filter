use clap::{Parser, Subcommand};
use serde::Serialize;
use sodola_switch::{AcceptedFrameType, SodolaClient, VlanPortMode};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{exit, Command};

const COOKIE_DIR: &str = ".local/share/nifty-filter/sodola-switch";
const COOKIE_FILE: &str = "credentials";

fn cookie_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(COOKIE_DIR).join(COOKIE_FILE)
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
        #[arg(long, env = "SODOLA_TRUNK_IFACE", default_value = "trunk")]
        iface: String,
        /// IP address to assign (in the switch's subnet)
        #[arg(long, env = "SODOLA_ROUTER_IP", default_value = "192.168.2.2/24")]
        ip: String,
    },
    /// Remove IP address from trunk interface (requires sudo)
    RouteDown {
        /// Network interface connected to the switch
        #[arg(long, env = "SODOLA_TRUNK_IFACE", default_value = "trunk")]
        iface: String,
        /// IP address to remove
        #[arg(long, env = "SODOLA_ROUTER_IP", default_value = "192.168.2.2/24")]
        ip: String,
    },
    /// Log out and remove saved session
    Logout,
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

fn main() {
    let cli = Cli::parse();
    let mut client = SodolaClient::new(&cli.url);

    // Route commands run ip(8) directly — no switch connection needed
    match &cli.command {
        Commands::RouteUp { iface, ip } => {
            let status = Command::new("sudo")
                .args(["ip", "addr", "add", ip, "dev", iface])
                .status();
            match status {
                Ok(s) if s.success() => {
                    eprintln!("Added {} to {}", ip, iface);
                    return;
                }
                Ok(s) => {
                    // exit code 2 = already exists, treat as success
                    if s.code() == Some(2) {
                        eprintln!("{} already has {}", iface, ip);
                        return;
                    }
                    eprintln!("Failed to add {} to {} (exit {})", ip, iface, s);
                    exit(1);
                }
                Err(e) => {
                    eprintln!("Failed to run sudo: {}", e);
                    exit(1);
                }
            }
        }
        Commands::RouteDown { iface, ip } => {
            let status = Command::new("sudo")
                .args(["ip", "addr", "del", ip, "dev", iface])
                .status();
            match status {
                Ok(s) if s.success() => {
                    eprintln!("Removed {} from {}", ip, iface);
                    return;
                }
                Ok(_) => {
                    eprintln!("{} not present on {}", ip, iface);
                    return;
                }
                Err(e) => {
                    eprintln!("Failed to run sudo: {}", e);
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
        Commands::RouteUp { .. } | Commands::RouteDown { .. } => unreachable!(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        exit(1);
    }
}
