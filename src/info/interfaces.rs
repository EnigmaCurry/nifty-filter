use pnet::datalink;
use serde::Serialize;
use std::fs;
use std::io::{self, Error, ErrorKind};
use std::net::IpAddr;
use std::path::Path;
use std::process::Command;
use strum::Display;

use super::pretty_print_json;

#[derive(Debug, Serialize, Clone, Display, PartialEq)]
pub enum InterfaceType {
    Loopback,
    Bridge,
    PhysicalEthernet,
    PhysicalWifi,
    Virtual,
    Tap,
    Unknown,
}

impl InterfaceType {
    pub fn as_str(&self) -> &str {
        match self {
            InterfaceType::Loopback => "Loopback",
            InterfaceType::PhysicalEthernet => "Physical Ethernet",
            InterfaceType::PhysicalWifi => "Physical WiFi",
            InterfaceType::Virtual => "Virtual Ethernet",
            InterfaceType::Tap => "Tap device",
            InterfaceType::Bridge => "Bridge",
            InterfaceType::Unknown => "Unknown",
        }
    }
}

pub const MANAGED_INTERFACE_TYPES: &[InterfaceType] = &[
    InterfaceType::PhysicalEthernet,
    InterfaceType::PhysicalWifi,
    InterfaceType::Virtual,
];

#[derive(Serialize, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub status: String,
    pub mtu: u32,
    pub state: String,
    pub group: String,
    pub interface_type: InterfaceType,
    pub tx_queue_len: u32,
    pub types: Vec<String>,
    pub mac_address: Option<String>,
    pub ip_addresses: Vec<String>,
    pub pci_info: String,
}

pub fn get_mtu(interface: &str) -> io::Result<u32> {
    let path = Path::new("/sys/class/net").join(interface).join("mtu");
    let mtu_str = fs::read_to_string(&path)?;
    let mtu = mtu_str.trim().parse::<u32>().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to parse MTU: {}", e),
        )
    })?;
    Ok(mtu)
}

pub fn get_pci_info(interface: &str) -> io::Result<String> {
    // Read vendor and device ID
    let base_path = format!("/sys/class/net/{}/device", interface);
    let vendor = std::fs::read_to_string(format!("{}/vendor", base_path))?
        .trim()
        .to_string();
    let device = std::fs::read_to_string(format!("{}/device", base_path))?
        .trim()
        .to_string();

    // Remove the "0x" prefix and build the device ID format
    let vendor_id = vendor.trim_start_matches("0x");
    let device_id = device.trim_start_matches("0x");
    let device_str = format!("{}:{}", vendor_id, device_id);

    // Use `lspci` to get a human-readable name for the device
    let output = Command::new("lspci")
        .args(&["-nnk", "-d", &device_str])
        .output()?;

    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other, "Failed to run lspci command"));
    }

    // Parse the output to extract the device name
    let stdout = String::from_utf8_lossy(&output.stdout);
    let model_name = stdout
        .lines()
        .find(|line| line.contains(&device_str))
        .map(|line| line.to_string())
        .unwrap_or_else(|| format!("Unknown device for {}", device_str));

    Ok(model_name)
}

pub fn get_state(interface: &str) -> io::Result<String> {
    let path = Path::new("/sys/class/net")
        .join(interface)
        .join("operstate");
    fs::read_to_string(&path).map(|state| state.trim().to_string())
}

pub fn get_group(interface: &str) -> io::Result<String> {
    let path = Path::new("/sys/class/net")
        .join(interface)
        .join("phys_port_name");
    fs::read_to_string(&path).map(|group| group.trim().to_string())
}

pub fn get_tx_queue_len(interface: &str) -> io::Result<u32> {
    let path = Path::new("/sys/class/net")
        .join(interface)
        .join("tx_queue_len");
    let qlen_str = fs::read_to_string(&path)?;
    let qlen = qlen_str.trim().parse::<u32>().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to parse TX queue length: {}", e),
        )
    })?;
    Ok(qlen)
}

pub fn get_interfaces() -> Vec<InterfaceInfo> {
    datalink::interfaces()
        .into_iter()
        .map(|iface| get_interface(&iface))
        .collect()
}

pub fn get_interface(iface: &datalink::NetworkInterface) -> InterfaceInfo {
    let is_up = iface.is_up();
    let is_broadcast = iface.is_broadcast();
    let is_multicast = iface.is_multicast();
    let is_loopback = iface.is_loopback();
    let is_point_to_point = iface.is_point_to_point();

    let mtu = get_mtu(&iface.name).unwrap_or(0);
    let state = get_state(&iface.name).unwrap_or_else(|_| "Unknown".to_string());
    let group = get_group(&iface.name).unwrap_or_else(|_| "default".to_string());
    let qlen = get_tx_queue_len(&iface.name).unwrap_or(0);
    let pci_info = get_pci_info(&iface.name).unwrap_or_else(|_| "Unknown".to_string());
    let interface_type;

    if is_loopback {
        interface_type = InterfaceType::Loopback;
    } else {
        let base_path = Path::new("/sys/class/net").join(iface.name.clone());
        // Check for "device" directory to determine physical vs virtual
        let device_path = base_path.join("device");
        if device_path.exists() {
            // Presence of "device" suggests physical interface
            if Path::new(&base_path.join("wireless")).exists() {
                interface_type = InterfaceType::PhysicalWifi;
            } else {
                interface_type = InterfaceType::PhysicalEthernet;
            }
        } else {
            // Absence of "device" suggests virtual interface
            let if_tun_path = base_path.join("tun_flags");
            let if_bridge_path = base_path.join("bridge");
            if if_tun_path.exists() {
                interface_type = InterfaceType::Tap;
            } else if if_bridge_path.exists() {
                interface_type = InterfaceType::Bridge;
            } else {
                interface_type = InterfaceType::Virtual;
            }
        }
    }

    let mut types = Vec::new();
    if is_broadcast {
        types.push("Broadcast".to_string());
    }
    if is_multicast {
        types.push("Multicast".to_string());
    }
    if is_loopback {
        types.push("Loopback".to_string());
    }
    if is_point_to_point {
        types.push("Point-to-Point".to_string());
    }

    let mac_address = iface.mac.map(|mac| mac.to_string());
    let ip_addresses = iface
        .ips
        .iter()
        .map(|ip| match ip.ip() {
            IpAddr::V4(ipv4) => ipv4.to_string(),
            IpAddr::V6(ipv6) => ipv6.to_string(),
        })
        .collect();

    InterfaceInfo {
        name: iface.name.clone(),
        status: if is_up {
            "Up".to_string()
        } else {
            "Down".to_string()
        },
        mtu,
        state,
        group,
        pci_info,
        interface_type,
        tx_queue_len: qlen,
        types,
        mac_address,
        ip_addresses,
    }
}

pub fn interfaces() -> io::Result<()> {
    let mut interfaces_info = Vec::new();

    for iface in datalink::interfaces() {
        interfaces_info.push(get_interface(&iface));
    }

    let json_value = serde_json::to_value(&interfaces_info)?;
    pretty_print_json(json_value);

    Ok(())
}
