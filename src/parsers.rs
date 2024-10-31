use std::env;

pub mod chain_policy;
pub mod icmp_type;
pub mod interface;
pub mod port;
pub mod subnet;

pub use chain_policy::ChainPolicy;
pub use icmp_type::IcmpType;
pub use interface::Interface;
#[allow(unused_imports)]
pub use port::Port;
pub use subnet::Subnet;

use self::port::PortList;

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

pub fn get_chain_policy(
    var_name: &str,
    errors: &mut Vec<String>,
    default: ChainPolicy,
) -> ChainPolicy {
    let val = match get_string_var(var_name) {
        Ok(val) => val,
        Err(_) => return default,
    };

    match ChainPolicy::new(&val) {
        Ok(policy) => policy,
        Err(err) => {
            errors.push(err);
            println!("Invalid chain policy value for {}: {}", var_name, val);
            ChainPolicy::Drop // Dummy value
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
        Ok(val) => val
            .split(',')
            .filter_map(|s| match IcmpType::new(s.trim()) {
                Ok(icmp_type) => Some(icmp_type),
                Err(err) => {
                    errors.push(err);
                    None
                }
            })
            .collect(),
        Err(_) => default,
    }
}

/// Gets a `PortList` from an environment variable, or returns a default.
pub fn get_port_accept(var_name: &str, _errors: &mut Vec<String>, default: PortList) -> PortList {
    match get_string_var(var_name) {
        Ok(val) => match PortList::new(&val) {
            Ok(port_list) => port_list,
            Err(_) => default,
        },
        Err(_) => default,
    }b
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
                Some(default) => return default,
                None => {
                    errors.push(err);
                    false // Dummy value
                }
            }
        }
    }
}
