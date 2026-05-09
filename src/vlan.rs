use std::collections::HashSet;

use crate::parsers::forward_route::ForwardRouteList;
use crate::parsers::inbound_rule::InboundRuleList;
use crate::parsers::inter_vlan_rule::InterVlanRuleList;
use crate::parsers::qos_class::QosClass;

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
    pub qos_class: Option<QosClass>,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
