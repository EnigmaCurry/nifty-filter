use serde::Deserialize;
use std::collections::HashMap;

/// Top-level HCL configuration.
#[derive(Debug, Deserialize)]
pub struct HclConfig {
    pub interfaces: InterfacesConfig,
    pub wan: WanConfig,
    #[serde(default)]
    pub dns: Option<DnsConfig>,
    #[serde(default)]
    pub qos: Option<QosHclConfig>,
    #[serde(default)]
    pub vlan: HashMap<String, VlanHclConfig>,
}

#[derive(Debug, Deserialize)]
pub struct InterfacesConfig {
    pub trunk: String,
    pub wan: String,
    #[serde(default)]
    pub mgmt: Option<String>,
    #[serde(default)]
    pub mgmt_subnet: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WanConfig {
    #[serde(default = "default_true")]
    pub enable_ipv4: bool,
    #[serde(default)]
    pub enable_ipv6: bool,
    #[serde(default)]
    pub icmp_accept: Vec<String>,
    #[serde(default)]
    pub icmpv6_accept: Vec<String>,
    #[serde(default)]
    pub tcp_accept: Vec<u16>,
    #[serde(default)]
    pub udp_accept: Vec<u16>,
    #[serde(default)]
    pub tcp_forward: Vec<String>,
    #[serde(default)]
    pub udp_forward: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct DnsConfig {
    pub upstream: Vec<String>,
}

/// Per-VLAN configuration block.
#[derive(Debug, Deserialize)]
pub struct VlanHclConfig {
    pub id: u16,
    #[serde(default)]
    pub ipv4: Option<Ipv4Config>,
    #[serde(default)]
    pub ipv6: Option<Ipv6Config>,
    #[serde(default)]
    pub firewall: Option<FirewallConfig>,
    #[serde(default)]
    pub dhcp: Option<DhcpConfig>,
    #[serde(default)]
    pub dhcpv6: Option<Dhcpv6Config>,
    #[serde(default)]
    pub qos_class: Option<String>,
    #[serde(default)]
    pub iperf_enabled: bool,
    #[serde(default)]
    pub allow_inbound_tcp: Vec<String>,
    #[serde(default)]
    pub allow_inbound_udp: Vec<String>,
    #[serde(default)]
    pub allow_from: HashMap<String, InterVlanHclConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Ipv4Config {
    pub subnet: String,
    #[serde(default)]
    pub egress: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Ipv6Config {
    pub subnet: String,
    #[serde(default)]
    pub egress: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FirewallConfig {
    #[serde(default)]
    pub icmp_accept: Vec<String>,
    #[serde(default)]
    pub icmpv6_accept: Vec<String>,
    #[serde(default)]
    pub tcp_accept: Vec<u16>,
    #[serde(default)]
    pub udp_accept: Vec<u16>,
}

#[derive(Debug, Deserialize)]
pub struct DhcpConfig {
    pub pool_start: String,
    pub pool_end: String,
    pub router: String,
    pub dns: String,
    #[serde(default)]
    pub host: Vec<DhcpHost>,
}

#[derive(Debug, Deserialize)]
pub struct DhcpHost {
    pub mac: String,
    pub ip: String,
    #[serde(default)]
    pub hostname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Dhcpv6Config {
    pub pool_start: String,
    pub pool_end: String,
}

#[derive(Debug, Deserialize)]
pub struct InterVlanHclConfig {
    #[serde(default)]
    pub tcp: Vec<String>,
    #[serde(default)]
    pub udp: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct QosHclConfig {
    pub upload_mbps: u32,
    pub download_mbps: u32,
    #[serde(default = "default_shave")]
    pub shave_percent: u8,
    #[serde(default)]
    pub overrides: Option<QosOverridesConfig>,
}

fn default_shave() -> u8 {
    10
}

#[derive(Debug, Deserialize)]
pub struct QosOverridesConfig {
    #[serde(default)]
    pub voice: Vec<String>,
    #[serde(default)]
    pub video: Vec<String>,
    #[serde(default)]
    pub besteffort: Vec<String>,
    #[serde(default)]
    pub bulk: Vec<String>,
}

/// Parse an HCL configuration string into an HclConfig.
pub fn parse_hcl(input: &str) -> Result<HclConfig, String> {
    hcl::from_str(input).map_err(|e| format!("HCL parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Test 1: Verify hcl-rs can deserialize labeled blocks as HashMap
    // ---------------------------------------------------------------
    #[test]
    fn test_labeled_blocks_as_hashmap() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {
                enable_ipv4 = true
            }
            vlan "a" {
                id = 10
            }
            vlan "b" {
                id = 20
            }
        "#;
        let config = parse_hcl(input).unwrap();
        assert_eq!(config.vlan.len(), 2);
        assert_eq!(config.vlan.get("a").unwrap().id, 10);
        assert_eq!(config.vlan.get("b").unwrap().id, 20);
    }

    // ---------------------------------------------------------------
    // Test 2: Minimal config — interfaces + WAN + one VLAN
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_minimal() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {
                enable_ipv4 = true
            }
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.99.10.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    icmp_accept = ["echo-request", "echo-reply"]
                    tcp_accept  = [22]
                    udp_accept  = [67, 68]
                }
                dhcp {
                    pool_start = "10.99.10.100"
                    pool_end   = "10.99.10.250"
                    router     = "10.99.10.1"
                    dns        = "10.99.10.1"
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();

        // Interfaces
        assert_eq!(config.interfaces.trunk, "trunk");
        assert_eq!(config.interfaces.wan, "wan");
        assert!(config.interfaces.mgmt.is_none());

        // WAN
        assert!(config.wan.enable_ipv4);
        assert!(!config.wan.enable_ipv6);
        assert!(config.wan.tcp_accept.is_empty());

        // VLAN
        let trusted = config.vlan.get("trusted").expect("missing trusted vlan");
        assert_eq!(trusted.id, 10);
        assert!(trusted.ipv6.is_none());
        assert!(trusted.dhcpv6.is_none());
        assert!(!trusted.iperf_enabled);

        // IPv4
        let ipv4 = trusted.ipv4.as_ref().unwrap();
        assert_eq!(ipv4.subnet, "10.99.10.1/24");
        assert_eq!(ipv4.egress, vec!["0.0.0.0/0"]);

        // Firewall
        let fw = trusted.firewall.as_ref().unwrap();
        assert_eq!(fw.icmp_accept, vec!["echo-request", "echo-reply"]);
        assert_eq!(fw.tcp_accept, vec![22]);
        assert_eq!(fw.udp_accept, vec![67, 68]);

        // DHCP
        let dhcp = trusted.dhcp.as_ref().unwrap();
        assert_eq!(dhcp.pool_start, "10.99.10.100");
        assert_eq!(dhcp.pool_end, "10.99.10.250");
        assert_eq!(dhcp.router, "10.99.10.1");
        assert_eq!(dhcp.dns, "10.99.10.1");
        assert!(dhcp.host.is_empty());
    }

    // ---------------------------------------------------------------
    // Test 3: Dual-stack VLAN with DHCPv6
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_dual_stack() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {
                enable_ipv4 = true
                enable_ipv6 = true
            }
            vlan "lab" {
                id = 40
                ipv4 {
                    subnet = "10.99.40.1/24"
                    egress = ["0.0.0.0/0"]
                }
                ipv6 {
                    subnet = "fd00:40::1/64"
                    egress = ["::/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68, 546, 547]
                }
                dhcp {
                    pool_start = "10.99.40.100"
                    pool_end   = "10.99.40.250"
                    router     = "10.99.40.1"
                    dns        = "10.99.40.1"
                }
                dhcpv6 {
                    pool_start = "fd00:40::100"
                    pool_end   = "fd00:40::1ff"
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let lab = config.vlan.get("lab").unwrap();

        // IPv6
        let ipv6 = lab.ipv6.as_ref().unwrap();
        assert_eq!(ipv6.subnet, "fd00:40::1/64");
        assert_eq!(ipv6.egress, vec!["::/0"]);

        // DHCPv6
        let dhcpv6 = lab.dhcpv6.as_ref().unwrap();
        assert_eq!(dhcpv6.pool_start, "fd00:40::100");
        assert_eq!(dhcpv6.pool_end, "fd00:40::1ff");
    }

    // ---------------------------------------------------------------
    // Test 4: WAN forward routes
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_wan_forward() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {
                tcp_forward = [
                    "443:10.99.40.50:443",
                    "22:10.99.40.10:22",
                ]
                udp_forward = []
            }
        "#;

        let config = parse_hcl(input).unwrap();
        assert_eq!(config.wan.tcp_forward.len(), 2);
        assert_eq!(config.wan.tcp_forward[0], "443:10.99.40.50:443");
        assert_eq!(config.wan.tcp_forward[1], "22:10.99.40.10:22");
        assert!(config.wan.udp_forward.is_empty());
    }

    // ---------------------------------------------------------------
    // Test 5: Inbound allow rules (IPv6 pinholing)
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_inbound_rules() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "lab" {
                id = 40
                allow_inbound_tcp = [
                    "443:[2001:db8:abcd:40::50]",
                    "22:[2001:db8:abcd:40::10]",
                ]
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let lab = config.vlan.get("lab").unwrap();
        assert_eq!(lab.allow_inbound_tcp.len(), 2);
        assert_eq!(lab.allow_inbound_tcp[0], "443:[2001:db8:abcd:40::50]");
        assert!(lab.allow_inbound_udp.is_empty());
    }

    // ---------------------------------------------------------------
    // Test 6: IoT jail — empty egress/tcp means deny-all
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_deny_all_vlan() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "iot" {
                id = 20
                ipv4 {
                    subnet = "10.99.20.1/24"
                    egress = []
                }
                firewall {
                    icmp_accept = ["destination-unreachable"]
                    tcp_accept  = []
                    udp_accept  = [67, 68]
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let iot = config.vlan.get("iot").unwrap();
        let ipv4 = iot.ipv4.as_ref().unwrap();
        assert!(ipv4.egress.is_empty());
        let fw = iot.firewall.as_ref().unwrap();
        assert!(fw.tcp_accept.is_empty());
        assert_eq!(fw.icmp_accept, vec!["destination-unreachable"]);
    }

    // ---------------------------------------------------------------
    // Test 7: Multiple VLANs (4-VLAN setup)
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_four_vlans() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "trusted" { id = 10 }
            vlan "iot"     { id = 20 }
            vlan "guest"   { id = 30 }
            vlan "lab"     { id = 40 }
        "#;

        let config = parse_hcl(input).unwrap();
        assert_eq!(config.vlan.len(), 4);

        // Verify all IDs are correct
        let mut ids: Vec<u16> = config.vlan.values().map(|v| v.id).collect();
        ids.sort();
        assert_eq!(ids, vec![10, 20, 30, 40]);

        // Verify name -> id mapping
        assert_eq!(config.vlan.get("trusted").unwrap().id, 10);
        assert_eq!(config.vlan.get("iot").unwrap().id, 20);
        assert_eq!(config.vlan.get("guest").unwrap().id, 30);
        assert_eq!(config.vlan.get("lab").unwrap().id, 40);
    }

    // ---------------------------------------------------------------
    // Test 8: DHCP static hosts
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_dhcp_hosts() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "trusted" {
                id = 10
                dhcp {
                    pool_start = "10.99.10.100"
                    pool_end   = "10.99.10.250"
                    router     = "10.99.10.1"
                    dns        = "10.99.10.1"

                    host {
                        mac      = "aa:bb:cc:dd:ee:01"
                        ip       = "10.99.10.10"
                        hostname = "server1"
                    }
                    host {
                        mac      = "aa:bb:cc:dd:ee:02"
                        ip       = "10.99.10.11"
                        hostname = "nas"
                    }
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let dhcp = config.vlan.get("trusted").unwrap().dhcp.as_ref().unwrap();
        assert_eq!(dhcp.host.len(), 2);
        assert_eq!(dhcp.host[0].mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(dhcp.host[0].ip, "10.99.10.10");
        assert_eq!(dhcp.host[0].hostname.as_deref(), Some("server1"));
        assert_eq!(dhcp.host[1].mac, "aa:bb:cc:dd:ee:02");
    }

    // ---------------------------------------------------------------
    // Test 9: QoS configuration
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_qos() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            qos {
                upload_mbps   = 20
                download_mbps = 300
                shave_percent = 15

                overrides {
                    voice = ["10.99.10.50", "10.99.10.51"]
                    bulk  = ["10.99.20.0/24"]
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let qos = config.qos.as_ref().unwrap();
        assert_eq!(qos.upload_mbps, 20);
        assert_eq!(qos.download_mbps, 300);
        assert_eq!(qos.shave_percent, 15);

        let overrides = qos.overrides.as_ref().unwrap();
        assert_eq!(overrides.voice, vec!["10.99.10.50", "10.99.10.51"]);
        assert_eq!(overrides.bulk, vec!["10.99.20.0/24"]);
        assert!(overrides.video.is_empty());
        assert!(overrides.besteffort.is_empty());
    }

    // ---------------------------------------------------------------
    // Test 10: QoS default shave percent
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_qos_default_shave() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            qos {
                upload_mbps   = 20
                download_mbps = 300
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let qos = config.qos.unwrap();
        assert_eq!(qos.shave_percent, 10);
    }

    // ---------------------------------------------------------------
    // Test 11: DNS upstream servers
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_dns() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            dns {
                upstream = ["1.1.1.1", "1.0.0.1"]
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let dns = config.dns.unwrap();
        assert_eq!(dns.upstream, vec!["1.1.1.1", "1.0.0.1"]);
    }

    // ---------------------------------------------------------------
    // Test 12: No DNS block is OK
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_no_dns() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
        "#;

        let config = parse_hcl(input).unwrap();
        assert!(config.dns.is_none());
    }

    // ---------------------------------------------------------------
    // Test 13: Inter-VLAN allow rules
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_inter_vlan_rules() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "trusted" {
                id = 10
            }
            vlan "lab" {
                id = 40
                allow_from "trusted" {
                    tcp = ["10.99.40.5:80", "10.99.10.50:10.99.40.5:443"]
                    udp = ["10.99.40.5:53"]
                }
            }
        "#;

        let config = parse_hcl(input).unwrap();
        let lab = config.vlan.get("lab").unwrap();
        let from_trusted = lab.allow_from.get("trusted").unwrap();
        assert_eq!(from_trusted.tcp.len(), 2);
        assert_eq!(from_trusted.tcp[0], "10.99.40.5:80");
        assert_eq!(from_trusted.tcp[1], "10.99.10.50:10.99.40.5:443");
        assert_eq!(from_trusted.udp, vec!["10.99.40.5:53"]);
    }

    // ---------------------------------------------------------------
    // Test 14: Management interface (optional)
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_mgmt_interface() {
        let input = r#"
            interfaces {
                trunk       = "trunk"
                wan         = "wan"
                mgmt        = "mgmt0"
                mgmt_subnet = "192.168.88.1/24"
            }
            wan {}
        "#;

        let config = parse_hcl(input).unwrap();
        assert_eq!(config.interfaces.mgmt.as_deref(), Some("mgmt0"));
        assert_eq!(config.interfaces.mgmt_subnet.as_deref(), Some("192.168.88.1/24"));
    }

    // ---------------------------------------------------------------
    // Test 15: Parse the full example file
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_full_example() {
        let input = include_str!("../examples/vlan_router.hcl");
        let config = parse_hcl(input).unwrap();

        // Interfaces
        assert_eq!(config.interfaces.trunk, "trunk");
        assert_eq!(config.interfaces.wan, "wan");

        // WAN
        assert!(config.wan.enable_ipv4);
        assert!(config.wan.enable_ipv6);
        assert_eq!(config.wan.tcp_forward.len(), 2);

        // DNS
        let dns = config.dns.unwrap();
        assert_eq!(dns.upstream, vec!["1.1.1.1", "1.0.0.1"]);

        // All four VLANs present
        assert_eq!(config.vlan.len(), 4);

        // Trusted VLAN
        let trusted = config.vlan.get("trusted").unwrap();
        assert_eq!(trusted.id, 10);
        let fw = trusted.firewall.as_ref().unwrap();
        assert_eq!(fw.tcp_accept, vec![22]);
        assert_eq!(fw.icmp_accept.len(), 4);

        // IoT VLAN — no egress
        let iot = config.vlan.get("iot").unwrap();
        assert_eq!(iot.id, 20);
        assert!(iot.ipv4.as_ref().unwrap().egress.is_empty());

        // Lab VLAN — dual-stack with DHCPv6
        let lab = config.vlan.get("lab").unwrap();
        assert_eq!(lab.id, 40);
        assert!(lab.ipv6.is_some());
        assert!(lab.dhcpv6.is_some());
        assert_eq!(lab.allow_inbound_tcp.len(), 2);

        // No QoS in the example (commented out)
        assert!(config.qos.is_none());
    }

    // ---------------------------------------------------------------
    // Test 16: VLAN with qos_class
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_vlan_qos_class() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "trusted" {
                id        = 10
                qos_class = "voice"
            }
            vlan "iot" {
                id        = 20
                qos_class = "bulk"
            }
        "#;

        let config = parse_hcl(input).unwrap();
        assert_eq!(
            config.vlan.get("trusted").unwrap().qos_class.as_deref(),
            Some("voice")
        );
        assert_eq!(
            config.vlan.get("iot").unwrap().qos_class.as_deref(),
            Some("bulk")
        );
    }

    // ---------------------------------------------------------------
    // Test 17: iperf enabled
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_iperf() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
            vlan "trusted" {
                id             = 10
                iperf_enabled  = true
            }
        "#;

        let config = parse_hcl(input).unwrap();
        assert!(config.vlan.get("trusted").unwrap().iperf_enabled);
    }

    // ---------------------------------------------------------------
    // Test 18: Invalid HCL produces error
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_invalid_hcl() {
        let input = r#"
            this is not valid {{{
        "#;
        assert!(parse_hcl(input).is_err());
    }

    // ---------------------------------------------------------------
    // Test 19: Missing required field produces error
    // ---------------------------------------------------------------
    #[test]
    fn test_parse_missing_interfaces() {
        let input = r#"
            wan {}
        "#;
        assert!(parse_hcl(input).is_err());
    }

    // ---------------------------------------------------------------
    // Test 20: WAN defaults — enable_ipv4 defaults to true
    // ---------------------------------------------------------------
    #[test]
    fn test_wan_defaults() {
        let input = r#"
            interfaces {
                trunk = "trunk"
                wan   = "wan"
            }
            wan {}
        "#;

        let config = parse_hcl(input).unwrap();
        assert!(config.wan.enable_ipv4);
        assert!(!config.wan.enable_ipv6);
        assert!(config.wan.icmp_accept.is_empty());
        assert!(config.wan.tcp_forward.is_empty());
    }
}
