use pnet::datalink;
use serde;
use serde::Serialize;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::Path;

use super::pretty_print_json;

#[derive(Serialize)]
struct InterfaceInfo {
    name: String,
    status: String,
    mtu: u32,
    state: String,
    group: String,
    tx_queue_len: u32,
    types: Vec<String>,
    mac_address: Option<String>,
    ip_addresses: Vec<String>,
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

pub fn network_info() -> io::Result<()> {
    let mut interfaces_info = Vec::new();

    for iface in datalink::interfaces() {
        let is_up = iface.is_up();
        let is_broadcast = iface.is_broadcast();
        let is_multicast = iface.is_multicast();
        let is_loopback = iface.is_loopback();
        let is_point_to_point = iface.is_point_to_point();

        let mtu = get_mtu(&iface.name).unwrap_or(0);
        let state = get_state(&iface.name).unwrap_or_else(|_| "unknown".to_string());
        let group = get_group(&iface.name).unwrap_or_else(|_| "default".to_string());
        let qlen = get_tx_queue_len(&iface.name).unwrap_or(0);

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

        interfaces_info.push(InterfaceInfo {
            name: iface.name.clone(),
            status: if is_up {
                "Up".to_string()
            } else {
                "Down".to_string()
            },
            mtu,
            state,
            group,
            tx_queue_len: qlen,
            types,
            mac_address,
            ip_addresses,
        });
    }

    let json_value = serde_json::to_value(&interfaces_info)?;
    pretty_print_json(json_value);

    Ok(())
}
