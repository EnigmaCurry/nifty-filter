use std::env;

pub mod dhcp_lease_time;
pub mod dhcp_static_leases;
pub mod domain;
pub mod forward_route;
pub mod icmp_type;
pub mod interface;
pub mod port;
pub mod subnet;

use self::port::PortList;
pub use dhcp_static_leases::{Lease, StaticLeases};
pub use forward_route::ForwardRouteList;
pub use icmp_type::IcmpType;
pub use interface::Interface;
#[allow(unused_imports)]
pub use port::Port;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
pub use subnet::Subnet;

fn get_string_var(var_name: &str) -> Result<String, String> {
    env::var(var_name).map_err(|_| format!("{} environment variable is not set.", var_name))
}

pub fn get_interface(var_name: &str, errors: &mut Vec<String>) -> Interface {
    match get_string_var(var_name) {
        Ok(val) => match Interface::new(&val) {
            Ok(interface) => interface,
            Err(err) => {
                errors.push(err);
                Interface::new("eth0").unwrap() // Dummy value
            }
        },
        Err(err) => {
            errors.push(err);
            Interface::new("eth0").unwrap() // Dummy value
        }
    }
}

pub fn get_subnet(var_name: &str, errors: &mut Vec<String>) -> Subnet {
    match get_string_var(var_name) {
        Ok(val) => match Subnet::new(&val) {
            Ok(subnet) => subnet,
            Err(err) => {
                errors.push(err);
                Subnet::new("0.0.0.0/32").unwrap() // Dummy value
            }
        },
        Err(err) => {
            errors.push(err);
            Subnet::new("0.0.0.0/32").unwrap() // Dummy value
        }
    }
}

pub fn get_icmp_types(
    var_name: &str,
    errors: &mut Vec<String>,
    default: Vec<IcmpType>,
) -> Vec<IcmpType> {
    match get_string_var(var_name) {
        Ok(val) => {
            if val.is_empty() {
                vec![]
            } else {
                val.split(',')
                    .filter_map(|s| match IcmpType::new(s.trim()) {
                        Ok(icmp_type) => Some(icmp_type),
                        Err(err) => {
                            errors.push(err);
                            None
                        }
                    })
                    .collect()
            }
        }
        Err(_) => default,
    }
}

/// Gets a `PortList` from an environment variable, or returns a default.
pub fn get_port_accept(var_name: &str, _errors: &mut [String], default: PortList) -> PortList {
    match get_string_var(var_name) {
        Ok(val) => match PortList::new(&val) {
            Ok(port_list) => port_list,
            Err(_) => default,
        },
        Err(_) => default,
    }
}

/// Gets a `ForwardRouteList` from an environment variable, or returns a default.
pub fn get_forward_routes(
    var_name: &str,
    _errors: &mut [String],
    default: ForwardRouteList,
) -> ForwardRouteList {
    match get_string_var(var_name) {
        Ok(val) => match ForwardRouteList::new(&val) {
            Ok(route_list) => route_list,
            Err(_) => default,
        },
        Err(_) => default,
    }
}

#[allow(dead_code)]
pub fn get_bool(var_name: &str, errors: &mut Vec<String>, default: Option<bool>) -> bool {
    match get_string_var(var_name) {
        Ok(val) => {
            if val == "true" {
                true
            } else if val == "false" {
                false
            } else {
                match default {
                    Some(default) => return default,
                    None => {
                        errors.push(format!("Invalid boolean variable: {var_name}={val}"));
                        false // Dummy value
                    }
                }
            }
        }
        Err(err) => {
            match default {
                Some(default) => default,
                None => {
                    errors.push(err);
                    false // Dummy value
                }
            }
        }
    }
}

pub fn get_ip_address(var_name: &str, errors: &mut Vec<String>) -> IpAddr {
    match get_string_var(var_name) {
        Ok(val) => {
            IpAddr::from_str(&val).unwrap_or_else(|_| panic!("Failed to parse IP address: {val}"))
        }
        Err(err) => {
            errors.push(err);
            IpAddr::V4(Ipv4Addr::new(127, 255, 255, 255)) // Dummy value
        }
    }
}

pub fn get_domain_name(var_name: &str, errors: &mut Vec<String>) -> domain::Domain {
    match get_string_var(var_name) {
        Ok(val) => {
            domain::Domain::new(&val).unwrap_or_else(|_| panic!("Invalid domain name: {val}"))
        }
        Err(err) => {
            errors.push(err);
            domain::Domain::new("invalid.example.com").unwrap() // Dummy value
        }
    }
}

pub fn get_dhcp_lease_time(
    var_name: &str,
    errors: &mut Vec<String>,
) -> dhcp_lease_time::DHCPLeaseTime {
    match get_string_var(var_name) {
        Ok(val) => dhcp_lease_time::parse_dhcp_lease_time(&val)
            .unwrap_or_else(|_| panic!("Invalid DHCP lease time: {val}")),
        Err(err) => {
            errors.push(err);
            dhcp_lease_time::DHCPLeaseTime::Infinite // Dummy value
        }
    }
}

pub fn get_static_leases(var_name: &str, errors: &mut Vec<String>) -> Vec<Lease> {
    match get_string_var(var_name) {
        Ok(val) => match StaticLeases::new(&val) {
            Ok(static_leases) => static_leases.get_leases().to_vec(),
            Err(err) => {
                errors.push(err);
                vec![]
            }
        },
        Err(err) => {
            errors.push(err);
            vec![]
        }
    }
}
