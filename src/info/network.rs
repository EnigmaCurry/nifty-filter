use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::BufRead;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinkFile {
    pub file_name: String,
    pub priority: i32,
    pub name: Option<String>,
    pub mac_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkFile {
    pub file_name: String,
    pub priority: i32,
    pub name: Option<String>,
    pub address: Option<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub kind: Option<String>,
    pub ip_masquerade: Option<String>,
    pub dhcp_server: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkInfo {
    pub links: HashMap<String, LinkFile>,
    pub networks: HashMap<String, NetworkFile>,
}

pub fn get_systemd_networks() -> io::Result<NetworkInfo> {
    let directories = vec![
        Path::new("/etc/systemd/network"),
        Path::new("/run/systemd/network"),
        Path::new("/usr/local/lib/systemd/network"),
        Path::new("/usr/lib/systemd/network"),
    ];

    let mut links_vec: Vec<LinkFile> = Vec::new();
    let mut networks_vec: Vec<NetworkFile> = Vec::new();

    // Collect files with priorities
    let mut files = Vec::new();
    for dir in directories {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() {
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                    if let Some(priority) = parse_priority_from_filename(&file_name) {
                        files.push((path, file_name, priority));
                    }
                }
            }
        }
    }

    // Sort files by priority
    files.sort_by_key(|(_, _, priority)| *priority);

    // Process each file in sorted order
    for (path, file_name, priority) in files {
        let full_path = path.to_string_lossy().to_string();

        if file_name.ends_with(".link") {
            let mut link_file = LinkFile {
                file_name: full_path.clone(),
                priority,
                name: None,
                mac_address: None,
            };
            parse_link_file(&path, &mut link_file);

            if link_file.name.is_some() {
                links_vec.push(link_file);
            }
        } else if file_name.ends_with(".network") {
            let mut network_file = NetworkFile {
                file_name: full_path.clone(),
                priority,
                name: None,
                address: None,
                gateway: None,
                dns: Vec::new(),
                kind: None,
                ip_masquerade: None,
                dhcp_server: None,
            };
            parse_network_file(&path, &mut network_file);

            if network_file.name.is_some() {
                networks_vec.push(network_file);
            }
        }
    }

    // Sort and insert into HashMaps while retaining order
    let mut links_map: HashMap<String, LinkFile> = HashMap::new();
    links_vec.sort_by_key(|lf| (lf.priority, lf.name.clone()));
    for link in links_vec {
        if let Some(name) = link.name.clone() {
            links_map.insert(name, link);
        }
    }

    let mut networks_map: HashMap<String, NetworkFile> = HashMap::new();
    networks_vec.sort_by_key(|nf| (nf.priority, nf.name.clone()));
    for network in networks_vec {
        if let Some(name) = network.name.clone() {
            networks_map.insert(name, network);
        }
    }

    // Return the structured data
    Ok(NetworkInfo {
        links: links_map,
        networks: networks_map,
    })
}

pub fn network() -> io::Result<()> {
    let network_info = get_systemd_networks()?;
    let json_output = serde_json::to_string_pretty(&network_info)?;
    println!("{}", json_output);
    Ok(())
}

fn parse_priority_from_filename(file_name: &str) -> Option<i32> {
    file_name
        .split('-')
        .next()
        .and_then(|prefix| prefix.parse::<i32>().ok())
}

fn parse_link_file(path: &Path, link_file: &mut LinkFile) {
    if let Ok(file) = fs::File::open(path) {
        for line in io::BufReader::new(file).lines() {
            if let Ok(line) = line {
                if line.starts_with("Name=") {
                    link_file.name = line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("MACAddress=") {
                    link_file.mac_address = line.split('=').nth(1).map(|s| s.trim().to_string());
                }
            }
        }
    }
}

fn parse_network_file(path: &Path, network_file: &mut NetworkFile) {
    if let Ok(file) = fs::File::open(path) {
        for line in io::BufReader::new(file).lines() {
            if let Ok(line) = line {
                if line.starts_with("Name=") {
                    network_file.name = line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("Address=") {
                    network_file.address = line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("Gateway=") {
                    network_file.gateway = line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("DNS=") {
                    if let Some(dns_entry) = line.split('=').nth(1) {
                        network_file.dns.push(dns_entry.trim().to_string());
                    }
                } else if line.starts_with("Kind=") {
                    network_file.kind = line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("IPMasquerade=") {
                    network_file.ip_masquerade =
                        line.split('=').nth(1).map(|s| s.trim().to_string());
                } else if line.starts_with("DHCPServer=") {
                    network_file.dhcp_server = line.split('=').nth(1).map(|s| s.trim().to_string());
                }
            }
        }
    }
}
