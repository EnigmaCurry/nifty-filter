use std::env;

pub mod cidr_list;
pub mod forward_route;
pub mod icmp_type;
pub mod icmpv6_type;
pub mod inbound_rule;
pub mod inter_vlan_rule;
pub mod interface;
pub mod port;
pub mod subnet;

use self::port::PortList;
pub use cidr_list::CidrList;
pub use forward_route::ForwardRouteList;
pub use icmp_type::IcmpType;
pub use icmpv6_type::Icmpv6Type;
pub use inbound_rule::InboundRuleList;
pub use interface::Interface;
#[allow(unused_imports)]
pub use port::Port;
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

#[allow(dead_code)]
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

/// Gets a `Subnet` from an environment variable, returning None if unset.
/// Pushes an error only if the value is set but invalid.
pub fn get_subnet_optional(var_name: &str, errors: &mut Vec<String>) -> Option<Subnet> {
    match get_string_var(var_name) {
        Ok(val) => {
            if val.is_empty() {
                None
            } else {
                match Subnet::new(&val) {
                    Ok(subnet) => Some(subnet),
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }
            }
        }
        Err(_) => None,
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

pub fn get_icmpv6_types(
    var_name: &str,
    errors: &mut Vec<String>,
    default: Vec<Icmpv6Type>,
) -> Vec<Icmpv6Type> {
    match get_string_var(var_name) {
        Ok(val) => {
            if val.is_empty() {
                vec![]
            } else {
                val.split(',')
                    .filter_map(|s| match Icmpv6Type::new(s.trim()) {
                        Ok(icmpv6_type) => Some(icmpv6_type),
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

/// Gets an `InboundRuleList` from an environment variable, or returns a default.
pub fn get_inbound_rules(
    var_name: &str,
    _errors: &mut [String],
    default: InboundRuleList,
) -> InboundRuleList {
    match get_string_var(var_name) {
        Ok(val) => match InboundRuleList::new(&val) {
            Ok(list) => list,
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

/// Gets a `CidrList` from an environment variable, or returns a default.
pub fn get_cidr_list(var_name: &str, errors: &mut Vec<String>, default: CidrList) -> CidrList {
    match get_string_var(var_name) {
        Ok(val) => {
            if val.is_empty() {
                CidrList::new("").unwrap()
            } else {
                match CidrList::new(&val) {
                    Ok(cidr_list) => cidr_list,
                    Err(err) => {
                        errors.push(err);
                        default
                    }
                }
            }
        }
        Err(_) => default,
    }
}

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
