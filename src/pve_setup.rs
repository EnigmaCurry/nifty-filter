use std::process::{self, Command, Stdio};

use inquire::{InquireError, Select, Text};

fn die(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    process::exit(1);
}

fn ssh_cmd(ssh_opts: &str, remote: &str, cmd: &str) -> String {
    let mut args: Vec<&str> = ssh_opts.split_whitespace().collect();
    args.push(remote);
    args.push(cmd);
    Command::new("ssh")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn prompt_text(message: &str, default: &str) -> String {
    let mut prompt = Text::new(message);
    if !default.is_empty() {
        prompt = prompt.with_default(default);
    }
    match prompt.prompt() {
        Ok(val) => val,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            eprintln!("Aborted.");
            process::exit(1);
        }
        Err(e) => die(&format!("Prompt error: {e}")),
    }
}

fn prompt_select(message: &str, options: Vec<String>) -> String {
    match Select::new(message, options).prompt() {
        Ok(choice) => choice,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            eprintln!("Aborted.");
            process::exit(1);
        }
        Err(e) => die(&format!("Prompt error: {e}")),
    }
}

fn prompt_confirm(message: &str, default: bool) -> bool {
    let dflt = if default { "Y/n" } else { "y/N" };
    let answer = prompt_text(&format!("{message} ({dflt})"), if default { "y" } else { "n" });
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

struct PciDevice {
    id: String,
    mac: String,
    description: String,
}

impl std::fmt::Display for PciDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mac.is_empty() {
            write!(f, "{} {}", self.id, self.description)
        } else {
            write!(f, "{} [{}] {}", self.id, self.mac, self.description)
        }
    }
}

struct Bridge {
    name: String,
    mac: String,
}

impl std::fmt::Display for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mac.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{} [{}]", self.name, self.mac)
        }
    }
}

fn list_pci_devices(ssh_opts: &str, remote: &str) -> Vec<PciDevice> {
    let output = ssh_cmd(
        ssh_opts,
        remote,
        r#"lspci -Dnn | grep -i 'ethernet\|network' | while IFS= read -r line; do
            pci=$(echo "$line" | awk '{print $1}')
            desc=$(echo "$line" | cut -d' ' -f2-)
            mac=$(cat /sys/bus/pci/devices/$pci/net/*/address 2>/dev/null | head -1)
            printf '%s\t%s\t%s\n' "$pci" "$mac" "$desc"
        done"#,
    );
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.is_empty() || parts[0].is_empty() {
                return None;
            }
            Some(PciDevice {
                id: parts[0].to_string(),
                mac: parts.get(1).unwrap_or(&"").to_string(),
                description: parts.get(2).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

fn list_bridges(ssh_opts: &str, remote: &str) -> Vec<Bridge> {
    let output = ssh_cmd(
        ssh_opts,
        remote,
        "ip -br link show type bridge | awk '{print $1, $3}'",
    );
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                return None;
            }
            Some(Bridge {
                name: parts[0].to_string(),
                mac: parts.get(1).unwrap_or(&"").to_string(),
            })
        })
        .collect()
}

/// Returns (nic_id, mac) — mac may be empty if unknown
fn pick_nic(label: &str, ssh_opts: &str, remote: &str) -> (String, String) {
    let nic_type = prompt_select(
        &format!("{label} NIC type:"),
        vec!["Virtual (bridge)".to_string(), "PCI passthrough".to_string()],
    );

    if nic_type.starts_with("PCI") {
        let devices = list_pci_devices(ssh_opts, remote);
        if devices.is_empty() {
            eprintln!("No PCI network devices found on host.");
            let id = prompt_text("Enter PCI device ID manually (e.g. 57:00.0):", "");
            if id.is_empty() {
                die("No PCI device specified");
            }
            return (id, String::new());
        }
        let options: Vec<String> = devices.iter().map(|d| d.to_string()).collect();
        let choice = prompt_select(&format!("{label} PCI device:"), options);
        // Find the selected device to get its MAC
        let full_id = choice.split_whitespace().next().unwrap_or("").to_string();
        let mac = devices
            .iter()
            .find(|d| d.id == full_id)
            .map(|d| d.mac.clone())
            .unwrap_or_default();
        // Strip 0000: domain prefix if present for shorter output
        let short_id = full_id
            .strip_prefix("0000:")
            .unwrap_or(&full_id)
            .to_string();
        (short_id, mac)
    } else {
        let bridges = list_bridges(ssh_opts, remote);
        if bridges.is_empty() {
            let name = prompt_text("No bridges found. Enter bridge name manually:", "vmbr0");
            if name.is_empty() {
                die("No bridge specified");
            }
            return (name, String::new());
        }
        let options: Vec<String> = bridges.iter().map(|b| b.to_string()).collect();
        let choice = prompt_select(&format!("{label} bridge:"), options);
        // Extract bridge name (first token) — MAC not useful for bridges (assigned at VM creation)
        let name = choice.split_whitespace().next().unwrap_or("").to_string();
        (name, String::new())
    }
}

/// Interactive PVE setup wizard. Outputs shell variables to stdout.
pub fn run(pve_host: &str) {
    let remote = format!("root@{pve_host}");
    let ssh_opts = "-o ControlMaster=auto -o ControlPath=/tmp/nifty-pve-setup-%C -o ControlPersist=60";

    // Open persistent SSH connection
    eprintln!("Connecting to {pve_host}...");
    let status = Command::new("ssh")
        .args(ssh_opts.split_whitespace())
        .args(["-fN", &remote])
        .status();
    if status.map(|s| !s.success()).unwrap_or(true) {
        die(&format!("Failed to connect to {pve_host}"));
    }

    // Query used VM IDs
    let used_ids_output = ssh_cmd(ssh_opts, &remote, "qm list 2>/dev/null | awk 'NR>1 {print $1}'");
    let used_ids: Vec<u32> = used_ids_output
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();

    // Find lowest unused ID starting at 100
    let mut default_vmid: u32 = 100;
    while used_ids.contains(&default_vmid) {
        default_vmid += 1;
    }

    if !used_ids.is_empty() {
        let ids_str: Vec<String> = used_ids.iter().map(|id| id.to_string()).collect();
        eprintln!("Used VM IDs: {}", ids_str.join(", "));
    }

    let vmid_str = prompt_text("VM ID:", &default_vmid.to_string());
    let vmid: u32 = vmid_str.trim().parse().unwrap_or_else(|_| die("Invalid VM ID"));
    if used_ids.contains(&vmid) {
        die(&format!("VM ID {vmid} is already in use"));
    }

    let vm_name = prompt_text("VM name:", "nifty-filter");

    // Collect NICs as (id, mac) pairs
    let mut nics: Vec<(String, String)> = Vec::new();

    eprintln!();
    let wan_nic = pick_nic("WAN", ssh_opts, &remote);
    nics.push(wan_nic);

    eprintln!();
    let trunk_nic = pick_nic("Trunk", ssh_opts, &remote);
    nics.push(trunk_nic);

    loop {
        eprintln!();
        if !prompt_confirm("Add another NIC?", false) {
            break;
        }
        let extra_nic = pick_nic("Additional", ssh_opts, &remote);
        nics.push(extra_nic);
    }

    // Summary
    eprintln!();
    eprintln!("=== Summary ===");
    eprintln!("  PVE Host: {pve_host}");
    eprintln!("  VM ID:    {vmid}");
    eprintln!("  VM Name:  {vm_name}");
    eprintln!("  NICs:     {}", nics.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(" "));
    eprintln!();

    if !prompt_confirm("Proceed with install?", true) {
        eprintln!("Aborted.");
        process::exit(1);
    }

    // Close the SSH control connection
    Command::new("ssh")
        .args(ssh_opts.split_whitespace())
        .args(["-O", "exit", &remote])
        .stderr(Stdio::null())
        .status()
        .ok();

    // Output shell variables to stdout for the Justfile to eval
    let wan_mac = &nics[0].1;
    let trunk_mac = &nics[1].1;
    println!("VMID={vmid}");
    println!("VM_NAME={vm_name}");
    println!("NICS=({})", nics.iter().map(|(id, _)| format!("\"{}\"", id)).collect::<Vec<_>>().join(" "));
    println!("PVE_WAN_MAC=\"{wan_mac}\"");
    println!("PVE_TRUNK_MAC=\"{trunk_mac}\"");
}
