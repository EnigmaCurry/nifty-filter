use std::collections::HashSet;
use std::env;

use crate::parsers::forward_route::ForwardRouteList;
use crate::parsers::icmp_type::IcmpType;
use crate::parsers::icmpv6_type::Icmpv6Type;
use crate::parsers::inbound_rule::InboundRuleList;
use crate::parsers::inter_vlan_rule::InterVlanRuleList;
use crate::parsers::port::PortList;
use crate::parsers::{
    get_bool, get_cidr_list, get_forward_routes, get_icmp_types, get_icmpv6_types,
    get_inbound_rules, get_port_accept,
};
use crate::parsers::cidr_list::CidrList;

/// A single LAN segment — either VLAN 1 (bare trunk) or a tagged sub-interface.
pub struct Vlan {
    pub id: u16,
    pub name: String,
    pub interface_name: String,
    pub subnet_ipv4: String,
    pub subnet_ipv6: String,
    pub egress_allowed_ipv4: String,
    pub egress_allowed_ipv6: String,
    pub icmp_accept: String,
    pub icmpv6_accept: String,
    pub tcp_accept: String,
    pub udp_accept: String,
    pub tcp_forward: ForwardRouteList,
    pub udp_forward: ForwardRouteList,
    pub tcp_allow_inbound: InboundRuleList,
    pub udp_allow_inbound: InboundRuleList,
    pub tcp_allow_inter_vlan: InterVlanRuleList,
    pub udp_allow_inter_vlan: InterVlanRuleList,
    pub iperf_enabled: bool,
    pub dhcp_enabled: bool,
    pub dhcp_pool_start: String,
    pub dhcp_pool_end: String,
    pub dhcp_router: String,
    pub dhcp_dns: String,
    pub dhcpv6_enabled: bool,
    pub dhcpv6_pool_start: String,
    pub dhcpv6_pool_end: String,
}

/// Parse comma-separated VLAN IDs, validating uniqueness and range.
pub fn parse_vlan_ids(input: &str, errors: &mut Vec<String>) -> Vec<u16> {
    if input.trim().is_empty() {
        return vec![];
    }
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for part in input.split(',') {
        let part = part.trim();
        match part.parse::<u16>() {
            Ok(id) if id == 0 || id > 4094 => {
                errors.push(format!("VLAN ID out of range (1-4094): {}", id));
            }
            Ok(id) => {
                if !seen.insert(id) {
                    errors.push(format!("Duplicate VLAN ID: {}", id));
                } else {
                    ids.push(id);
                }
            }
            Err(_) => {
                errors.push(format!("Invalid VLAN ID (not a number): '{}'", part));
            }
        }
    }
    ids
}

/// Helper to read an env var with a VLAN-prefixed key, e.g., VLAN_10_SUBNET_IPV4.
fn vlan_env(vlan_id: u16, suffix: &str) -> Option<String> {
    env::var(format!("VLAN_{}_{}", vlan_id, suffix)).ok()
}

/// Read a string env var for a VLAN, with a fallback default.
fn vlan_string(vlan_id: u16, suffix: &str, default: &str) -> String {
    vlan_env(vlan_id, suffix).unwrap_or_else(|| default.to_string())
}

/// Detect whether any VLAN_N_* env vars are present (for auto-detecting VLAN config).
fn has_any_vlan_vars() -> bool {
    env::vars().any(|(k, _)| {
        k.starts_with("VLAN_")
            && k.len() > 5
            && k[5..].contains('_')
            && k[5..].split('_').next().map_or(false, |id| id.parse::<u16>().is_ok())
    })
}

/// Extract VLAN IDs from VLAN_N_* env var names (e.g., VLAN_10_SUBNET_IPV4 -> 10).
fn detect_vlan_ids_from_env() -> Vec<u16> {
    let mut ids = HashSet::new();
    for (k, _) in env::vars() {
        if k.starts_with("VLAN_") && k.len() > 5 {
            let rest = &k[5..];
            if let Some(id_str) = rest.split('_').next() {
                if let Ok(id) = id_str.parse::<u16>() {
                    if id >= 1 && id <= 4094 {
                        ids.insert(id);
                    }
                }
            }
        }
    }
    let mut sorted: Vec<u16> = ids.into_iter().collect();
    sorted.sort();
    sorted
}

/// Map legacy flat LAN env vars to VLAN 1 env vars if no VLAN_* vars exist.
/// This enables backward compatibility with existing configs.
fn apply_legacy_aliases() {
    // Only apply if no VLAN_* vars are already set
    if has_any_vlan_vars() || env::var("VLANS").is_ok() {
        return;
    }

    let aliases = [
        // (legacy var, VLAN 1 var)
        ("SUBNET_LAN_IPV4", "VLAN_1_SUBNET_IPV4"),
        ("SUBNET_LAN", "VLAN_1_SUBNET_IPV4"), // older alias
        ("SUBNET_LAN_IPV6", "VLAN_1_SUBNET_IPV6"),
        ("LAN_EGRESS_ALLOWED_IPV4", "VLAN_1_EGRESS_ALLOWED_IPV4"),
        ("LAN_EGRESS_ALLOWED_IPV6", "VLAN_1_EGRESS_ALLOWED_IPV6"),
        ("ICMP_ACCEPT_LAN", "VLAN_1_ICMP_ACCEPT"),
        ("ICMPV6_ACCEPT_LAN", "VLAN_1_ICMPV6_ACCEPT"),
        ("TCP_ACCEPT_LAN", "VLAN_1_TCP_ACCEPT"),
        ("UDP_ACCEPT_LAN", "VLAN_1_UDP_ACCEPT"),
        ("TCP_FORWARD_LAN", "VLAN_1_TCP_FORWARD"),
        ("UDP_FORWARD_LAN", "VLAN_1_UDP_FORWARD"),
        ("DHCP_POOL_START", "VLAN_1_DHCP_POOL_START"),
        ("DHCP_POOL_END", "VLAN_1_DHCP_POOL_END"),
        ("DHCP_ROUTER", "VLAN_1_DHCP_ROUTER"),
        ("DHCP_DNS", "VLAN_1_DHCP_DNS"),
        ("DHCPV6_POOL_START", "VLAN_1_DHCPV6_POOL_START"),
        ("DHCPV6_POOL_END", "VLAN_1_DHCPV6_POOL_END"),
    ];

    for (legacy, vlan_var) in aliases {
        if let Ok(val) = env::var(legacy) {
            if env::var(vlan_var).is_err() {
                env::set_var(vlan_var, &val);
            }
        }
    }

    // Map IPERF_ENABLED
    if let Ok(val) = env::var("IPERF_ENABLED") {
        if env::var("VLAN_1_IPERF_ENABLED").is_err() {
            env::set_var("VLAN_1_IPERF_ENABLED", &val);
        }
    }

    // Map DHCP4_ENABLED / DHCPV6_ENABLED
    if let Ok(val) = env::var("DHCP4_ENABLED") {
        if env::var("VLAN_1_DHCP_ENABLED").is_err() {
            env::set_var("VLAN_1_DHCP_ENABLED", &val);
        }
    }
    if let Ok(val) = env::var("DHCPV6_ENABLED") {
        if env::var("VLAN_1_DHCPV6_ENABLED").is_err() {
            env::set_var("VLAN_1_DHCPV6_ENABLED", &val);
        }
    }

    // Map INTERFACE_LAN -> INTERFACE_TRUNK
    if env::var("INTERFACE_TRUNK").is_err() {
        if let Ok(val) = env::var("INTERFACE_LAN") {
            env::set_var("INTERFACE_TRUNK", &val);
        }
    }
}

/// Parse a single VLAN's config from env vars.
fn parse_single_vlan(
    vlan_id: u16,
    trunk_name: &str,
    enable_ipv4: bool,
    is_vlan_1: bool,
    errors: &mut Vec<String>,
) -> Vlan {
    let name = vlan_env(vlan_id, "NAME").unwrap_or_default();
    let interface_name = if !name.is_empty() {
        name.clone()
    } else if vlan_id == 1 {
        trunk_name.to_string()
    } else {
        format!("{}.{}", trunk_name, vlan_id)
    };

    // Subnets
    let subnet_ipv4 = if enable_ipv4 {
        let val = vlan_env(vlan_id, "SUBNET_IPV4").unwrap_or_default();
        if val.is_empty() {
            errors.push(format!(
                "VLAN_{}_SUBNET_IPV4 is required when ENABLE_IPV4=true.",
                vlan_id
            ));
        }
        val
    } else {
        String::new()
    };

    // IPv6 subnet is independent per-VLAN, not gated on WAN IPv6.
    // This allows mixed setups (e.g., WAN IPv4-only but LAN uses ULA IPv6).
    let subnet_ipv6 = vlan_env(vlan_id, "SUBNET_IPV6").unwrap_or_default();

    // Egress: VLAN 1 defaults to allow-all, others default to deny-all
    let egress_default_ipv4 = if is_vlan_1 { "0.0.0.0/0" } else { "" };
    let egress_default_ipv6 = if is_vlan_1 { "::/0" } else { "" };

    let egress_allowed_ipv4 = if enable_ipv4 {
        let raw = vlan_string(vlan_id, "EGRESS_ALLOWED_IPV4", egress_default_ipv4);
        if raw.is_empty() {
            String::new()
        } else {
            get_cidr_list(
                &format!("VLAN_{}_EGRESS_ALLOWED_IPV4", vlan_id),
                errors,
                CidrList::new(&raw).unwrap_or_else(|_| CidrList::new("").unwrap()),
            )
            .to_string()
        }
    } else {
        String::new()
    };

    let egress_allowed_ipv6 = if !subnet_ipv6.is_empty() {
        let raw = vlan_string(vlan_id, "EGRESS_ALLOWED_IPV6", egress_default_ipv6);
        if raw.is_empty() {
            String::new()
        } else {
            get_cidr_list(
                &format!("VLAN_{}_EGRESS_ALLOWED_IPV6", vlan_id),
                errors,
                CidrList::new(&raw).unwrap_or_else(|_| CidrList::new("").unwrap()),
            )
            .to_string()
        }
    } else {
        String::new()
    };

    // Router-local access: VLAN 1 gets full defaults, others get minimal
    let icmp_default: Vec<IcmpType> = if is_vlan_1 {
        vec![
            IcmpType::EchoRequest,
            IcmpType::EchoReply,
            IcmpType::DestinationUnreachable,
            IcmpType::TimeExceeded,
        ]
    } else {
        vec![IcmpType::DestinationUnreachable]
    };
    let icmp_accept = if enable_ipv4 {
        IcmpType::vec_to_string(&get_icmp_types(
            &format!("VLAN_{}_ICMP_ACCEPT", vlan_id),
            errors,
            icmp_default,
        ))
    } else {
        String::new()
    };

    let icmpv6_default: Vec<Icmpv6Type> = if is_vlan_1 {
        vec![
            Icmpv6Type::NdNeighborSolicit,
            Icmpv6Type::NdNeighborAdvert,
            Icmpv6Type::NdRouterSolicit,
            Icmpv6Type::NdRouterAdvert,
            Icmpv6Type::EchoRequest,
            Icmpv6Type::EchoReply,
        ]
    } else {
        vec![
            Icmpv6Type::NdNeighborSolicit,
            Icmpv6Type::NdNeighborAdvert,
            Icmpv6Type::DestinationUnreachable,
        ]
    };
    let icmpv6_accept = if !subnet_ipv6.is_empty() {
        Icmpv6Type::vec_to_string(&get_icmpv6_types(
            &format!("VLAN_{}_ICMPV6_ACCEPT", vlan_id),
            errors,
            icmpv6_default,
        ))
    } else {
        String::new()
    };

    // iperf3 server access
    let iperf_enabled = get_bool(
        &format!("VLAN_{}_IPERF_ENABLED", vlan_id),
        errors,
        Some(false),
    );

    // DHCP (read early so we can adjust UDP defaults)
    let dhcp_enabled = get_bool(
        &format!("VLAN_{}_DHCP_ENABLED", vlan_id),
        errors,
        Some(true),
    );
    let dhcp_pool_start = vlan_string(vlan_id, "DHCP_POOL_START", "");
    let dhcp_pool_end = vlan_string(vlan_id, "DHCP_POOL_END", "");
    let dhcp_router = vlan_string(vlan_id, "DHCP_ROUTER", "");
    let dhcp_dns = vlan_string(vlan_id, "DHCP_DNS", "");

    // DHCPv6
    let dhcpv6_enabled = get_bool(
        &format!("VLAN_{}_DHCPV6_ENABLED", vlan_id),
        errors,
        Some(false),
    );
    let dhcpv6_pool_start = vlan_string(vlan_id, "DHCPV6_POOL_START", "");
    let dhcpv6_pool_end = vlan_string(vlan_id, "DHCPV6_POOL_END", "");

    // TCP/UDP: VLAN 1 defaults to SSH(22) + DHCP, others default to DHCP only
    // Include DHCPv6 ports (546,547) when DHCPv6 is enabled for this VLAN
    let tcp_default = if is_vlan_1 { "22" } else { "" };
    let udp_default = if dhcpv6_enabled {
        "67,68,546,547"
    } else {
        "67,68"
    };

    let tcp_accept = get_port_accept(
        &format!("VLAN_{}_TCP_ACCEPT", vlan_id),
        errors,
        PortList::new(tcp_default).unwrap(),
    )
    .to_string();

    let udp_accept = get_port_accept(
        &format!("VLAN_{}_UDP_ACCEPT", vlan_id),
        errors,
        PortList::new(udp_default).unwrap(),
    )
    .to_string();

    // Port forwarding
    let tcp_forward = get_forward_routes(
        &format!("VLAN_{}_TCP_FORWARD", vlan_id),
        errors,
        ForwardRouteList::new("").unwrap(),
    );
    let udp_forward = get_forward_routes(
        &format!("VLAN_{}_UDP_FORWARD", vlan_id),
        errors,
        ForwardRouteList::new("").unwrap(),
    );

    // Inbound allow (firewall pinholing for public IPv6 / non-NAT)
    let tcp_allow_inbound = get_inbound_rules(
        &format!("VLAN_{}_ALLOW_INBOUND_TCP", vlan_id),
        errors,
        InboundRuleList::new("").unwrap(),
    );
    let udp_allow_inbound = get_inbound_rules(
        &format!("VLAN_{}_ALLOW_INBOUND_UDP", vlan_id),
        errors,
        InboundRuleList::new("").unwrap(),
    );

    Vlan {
        id: vlan_id,
        name,
        interface_name,
        subnet_ipv4,
        subnet_ipv6,
        egress_allowed_ipv4,
        egress_allowed_ipv6,
        icmp_accept,
        icmpv6_accept,
        tcp_accept,
        udp_accept,
        tcp_forward,
        udp_forward,
        tcp_allow_inbound,
        udp_allow_inbound,
        tcp_allow_inter_vlan: InterVlanRuleList::new(),
        udp_allow_inter_vlan: InterVlanRuleList::new(),
        iperf_enabled,
        dhcp_enabled,
        dhcp_pool_start,
        dhcp_pool_end,
        dhcp_router,
        dhcp_dns,
        dhcpv6_enabled,
        dhcpv6_pool_start,
        dhcpv6_pool_end,
    }
}

/// Parse all VLAN configuration from env vars.
///
/// Returns the trunk interface name, switch-aware flag, and list of VLANs.
/// Handles backward compatibility with legacy flat LAN vars.
pub fn parse_vlans_from_env(
    enable_ipv4: bool,
    errors: &mut Vec<String>,
) -> (String, bool, Vec<Vlan>) {
    // Apply legacy aliases before reading anything
    apply_legacy_aliases();

    let trunk_name = env::var("INTERFACE_TRUNK").unwrap_or_default();
    if trunk_name.is_empty() {
        errors.push("INTERFACE_TRUNK (or INTERFACE_LAN) is required.".to_string());
    }

    let vlan_aware_switch = get_bool("VLAN_AWARE_SWITCH", errors, Some(false));

    // Determine VLAN IDs: explicit VLANS= takes priority, then auto-detect
    // from VLAN_N_* env vars, then fall back to VLAN 1 (simple mode)
    let vlan_ids = match env::var("VLANS") {
        Ok(val) if !val.trim().is_empty() => parse_vlan_ids(&val, errors),
        _ => {
            let detected = detect_vlan_ids_from_env();
            if detected.is_empty() {
                vec![1]
            } else {
                detected
            }
        }
    };

    if vlan_ids.is_empty() {
        errors.push("At least one VLAN must be configured.".to_string());
    }

    // Validate: switch-aware mode forbids VLAN 1
    if vlan_aware_switch {
        for &id in &vlan_ids {
            if id == 1 {
                errors.push(
                    "VLAN 1 (untagged) is not allowed when VLAN_AWARE_SWITCH=true. All VLANs must have ID > 1."
                        .to_string(),
                );
                break;
            }
        }
    }

    let mut vlans: Vec<Vlan> = vlan_ids
        .iter()
        .map(|&id| {
            parse_single_vlan(id, &trunk_name, enable_ipv4, id == 1, errors)
        })
        .collect();

    // Parse inter-VLAN allow rules (VLAN_N_ALLOW_FROM_M_TCP/UDP).
    // Done after all VLANs are created so we can resolve source interface names.
    parse_inter_vlan_rules(&mut vlans, errors);

    (trunk_name, vlan_aware_switch, vlans)
}

/// Scan env for VLAN_N_ALLOW_FROM_M_TCP/UDP and populate each VLAN's inter-VLAN rule lists.
fn parse_inter_vlan_rules(vlans: &mut [Vlan], errors: &mut Vec<String>) {
    // Build a lookup: vlan_id -> interface_name
    let vlan_lookup: Vec<(u16, String)> = vlans
        .iter()
        .map(|v| (v.id, v.interface_name.clone()))
        .collect();

    for vlan in vlans.iter_mut() {
        for &(src_id, ref src_iface) in &vlan_lookup {
            if src_id == vlan.id {
                continue;
            }
            for (suffix, list) in [
                ("TCP", &mut vlan.tcp_allow_inter_vlan),
                ("UDP", &mut vlan.udp_allow_inter_vlan),
            ] {
                let var_name = format!("VLAN_{}_ALLOW_FROM_{}_{}", vlan.id, src_id, suffix);
                if let Ok(val) = env::var(&var_name) {
                    if !val.is_empty() {
                        if let Err(e) = list.add_entry(src_id, src_iface.clone(), &val) {
                            errors.push(format!("{}: {}", var_name, e));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;

    #[test]
    fn test_parse_vlan_ids_valid() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("10, 20, 30", &mut errors);
        assert!(errors.is_empty());
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn test_parse_vlan_ids_single() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("1", &mut errors);
        assert!(errors.is_empty());
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn test_parse_vlan_ids_empty() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("", &mut errors);
        assert!(errors.is_empty());
        assert!(ids.is_empty());
    }

    #[test]
    fn test_parse_vlan_ids_duplicate() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("10, 10, 20", &mut errors);
        assert_eq!(ids, vec![10, 20]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Duplicate"));
    }

    #[test]
    fn test_parse_vlan_ids_out_of_range() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("0, 4095", &mut errors);
        assert!(ids.is_empty());
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_parse_vlan_ids_invalid() {
        let mut errors = Vec::new();
        let ids = parse_vlan_ids("abc, 10", &mut errors);
        assert_eq!(ids, vec![10]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("not a number"));
    }

    #[test]
    fn test_vlan_interface_name() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // VLAN 1 uses bare trunk
        let mut errors = Vec::new();
        env::set_var("VLAN_1_SUBNET_IPV4", "192.168.1.1/24");
        let vlan = parse_single_vlan(1, "trunk", true, true, &mut errors);
        assert_eq!(vlan.interface_name, "trunk");
        env::remove_var("VLAN_1_SUBNET_IPV4");

        // VLAN 10 uses sub-interface by default
        env::remove_var("VLAN_10_NAME"); // clear any leaked state from other tests
        env::set_var("VLAN_10_SUBNET_IPV4", "10.10.0.1/24");
        let vlan = parse_single_vlan(10, "trunk", true, false, &mut errors);
        assert_eq!(vlan.interface_name, "trunk.10");
        assert_eq!(vlan.name, "");
        env::remove_var("VLAN_10_SUBNET_IPV4");

        // VLAN 20 with custom name
        env::set_var("VLAN_20_SUBNET_IPV4", "10.20.0.1/24");
        env::set_var("VLAN_20_NAME", "iot");
        let vlan = parse_single_vlan(20, "trunk", true, false, &mut errors);
        assert_eq!(vlan.interface_name, "iot");
        assert_eq!(vlan.name, "iot");
        env::remove_var("VLAN_20_SUBNET_IPV4");
        env::remove_var("VLAN_20_NAME");
    }

    #[test]
    fn test_vlan_1_defaults_allow_all_egress() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut errors = Vec::new();
        env::set_var("VLAN_1_SUBNET_IPV4", "192.168.1.1/24");
        let vlan = parse_single_vlan(1, "trunk", true, true, &mut errors);
        assert_eq!(vlan.egress_allowed_ipv4, "0.0.0.0/0");
        assert_eq!(vlan.tcp_accept, "22");
        env::remove_var("VLAN_1_SUBNET_IPV4");
    }

    #[test]
    fn test_vlan_non_1_defaults_deny_all_egress() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut errors = Vec::new();
        env::set_var("VLAN_20_SUBNET_IPV4", "10.20.0.1/24");
        let vlan = parse_single_vlan(20, "trunk", true, false, &mut errors);
        assert_eq!(vlan.egress_allowed_ipv4, "");
        assert_eq!(vlan.tcp_accept, "");
        assert_eq!(vlan.udp_accept, "67, 68");
        env::remove_var("VLAN_20_SUBNET_IPV4");
    }
}
