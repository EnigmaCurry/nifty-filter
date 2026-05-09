use serde::Deserialize;
use std::collections::HashMap;

/// Top-level HCL configuration.
#[derive(Debug, Deserialize)]
pub struct HclConfig {
    pub interfaces: InterfacesConfig,
    pub wan: WanConfig,
    #[serde(default)]
    pub vlan_aware_switch: bool,
    #[serde(default)]
    pub iperf_port: Option<u16>,
    #[serde(default)]
    pub dns: Option<DnsConfig>,
    #[serde(default)]
    pub qos: Option<QosHclConfig>,
    #[serde(default)]
    pub switch: Option<SwitchConfig>,
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
    pub tcp_forward: Vec<String>,
    #[serde(default)]
    pub udp_forward: Vec<String>,
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

/// Managed switch configuration (sodola-switch).
/// The HCL is the central config; the NixOS module extracts env vars for sodola-switch.
#[derive(Debug, Deserialize)]
pub struct SwitchConfig {
    pub url: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub pass: Option<String>,
    #[serde(default)]
    pub mgmt_iface: Option<String>,
    #[serde(default)]
    pub router_ip: Option<String>,
    /// Per-port settings (PVID, accepted frame type, VLAN membership)
    #[serde(default)]
    pub port: HashMap<String, SwitchPortConfig>,
}

#[derive(Debug, Deserialize)]
pub struct SwitchPortConfig {
    pub pvid: u16,
    pub accept: String,
    /// VLAN membership for this port: vlan_N = "U"|"T"
    /// VLANs not listed are implicitly "X" (not-member).
    #[serde(default)]
    pub vlans: HashMap<String, String>,
}

/// Parse an HCL configuration string into an HclConfig.
pub fn parse_hcl(input: &str) -> Result<HclConfig, String> {
    hcl::from_str(input).map_err(|e| format!("HCL parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: minimal valid HCL prefix for tests that don't care about interfaces/wan.
    fn hcl_prefix() -> &'static str {
        r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {}
"#
    }

    /// Helper: parse HCL with standard prefix prepended.
    fn parse_with_prefix(body: &str) -> HclConfig {
        let input = format!("{}{}", hcl_prefix(), body);
        parse_hcl(&input).unwrap()
    }

    #[test]
    fn test_labeled_blocks_as_hashmap() {
        let config = parse_with_prefix(r#"
vlan "a" { id = 10 }
vlan "b" { id = 20 }
"#);
        assert_eq!(config.vlan.len(), 2);
        assert_eq!(config.vlan.get("a").unwrap().id, 10);
        assert_eq!(config.vlan.get("b").unwrap().id, 20);
    }

    #[test]
    fn test_parse_minimal() {
        let config = parse_with_prefix(r#"
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
"#);
        assert_eq!(config.interfaces.trunk, "trunk");
        assert!(!config.vlan_aware_switch);

        let trusted = config.vlan.get("trusted").unwrap();
        assert_eq!(trusted.id, 10);
        assert!(trusted.ipv6.is_none());
        assert!(!trusted.iperf_enabled);

        let ipv4 = trusted.ipv4.as_ref().unwrap();
        assert_eq!(ipv4.subnet, "10.99.10.1/24");
        assert_eq!(ipv4.egress, vec!["0.0.0.0/0"]);

        let fw = trusted.firewall.as_ref().unwrap();
        assert_eq!(fw.tcp_accept, vec![22]);
        assert_eq!(fw.udp_accept, vec![67, 68]);

        let dhcp = trusted.dhcp.as_ref().unwrap();
        assert_eq!(dhcp.pool_start, "10.99.10.100");
        assert!(dhcp.host.is_empty());
    }

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
        assert_eq!(lab.ipv6.as_ref().unwrap().subnet, "fd00:40::1/64");
        assert_eq!(lab.dhcpv6.as_ref().unwrap().pool_start, "fd00:40::100");
    }

    #[test]
    fn test_parse_wan_forward() {
        let input = r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {
  tcp_forward = ["443:10.99.40.50:443", "22:10.99.40.10:22"]
  udp_forward = []
}
"#;
        let config = parse_hcl(input).unwrap();
        assert_eq!(config.wan.tcp_forward.len(), 2);
        assert!(config.wan.udp_forward.is_empty());
    }

    #[test]
    fn test_parse_inbound_rules() {
        let config = parse_with_prefix(r#"
vlan "lab" {
  id = 40
  allow_inbound_tcp = ["443:[2001:db8:abcd:40::50]", "22:[2001:db8:abcd:40::10]"]
}
"#);
        let lab = config.vlan.get("lab").unwrap();
        assert_eq!(lab.allow_inbound_tcp.len(), 2);
        assert!(lab.allow_inbound_udp.is_empty());
    }

    #[test]
    fn test_parse_deny_all_vlan() {
        let config = parse_with_prefix(r#"
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
"#);
        let iot = config.vlan.get("iot").unwrap();
        assert!(iot.ipv4.as_ref().unwrap().egress.is_empty());
        assert!(iot.firewall.as_ref().unwrap().tcp_accept.is_empty());
    }

    #[test]
    fn test_parse_four_vlans() {
        let config = parse_with_prefix(r#"
vlan "trusted" { id = 10 }
vlan "iot"     { id = 20 }
vlan "guest"   { id = 30 }
vlan "lab"     { id = 40 }
"#);
        assert_eq!(config.vlan.len(), 4);
        let mut ids: Vec<u16> = config.vlan.values().map(|v| v.id).collect();
        ids.sort();
        assert_eq!(ids, vec![10, 20, 30, 40]);
    }

    #[test]
    fn test_parse_dhcp_hosts() {
        let config = parse_with_prefix(r#"
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
"#);
        let dhcp = config.vlan.get("trusted").unwrap().dhcp.as_ref().unwrap();
        assert_eq!(dhcp.host.len(), 2);
        assert_eq!(dhcp.host[0].mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(dhcp.host[0].hostname.as_deref(), Some("server1"));
    }

    #[test]
    fn test_parse_qos() {
        let config = parse_with_prefix(r#"
qos {
  upload_mbps   = 20
  download_mbps = 300
  shave_percent = 15
  overrides {
    voice = ["10.99.10.50"]
    bulk  = ["10.99.20.0/24"]
  }
}
"#);
        let qos = config.qos.as_ref().unwrap();
        assert_eq!(qos.upload_mbps, 20);
        assert_eq!(qos.shave_percent, 15);
        let overrides = qos.overrides.as_ref().unwrap();
        assert_eq!(overrides.voice, vec!["10.99.10.50"]);
    }

    #[test]
    fn test_parse_qos_default_shave() {
        let config = parse_with_prefix(r#"
qos {
  upload_mbps   = 20
  download_mbps = 300
}
"#);
        assert_eq!(config.qos.unwrap().shave_percent, 10);
    }

    #[test]
    fn test_parse_dns() {
        let config = parse_with_prefix(r#"
dns { upstream = ["1.1.1.1", "1.0.0.1"] }
"#);
        assert_eq!(config.dns.unwrap().upstream, vec!["1.1.1.1", "1.0.0.1"]);
    }

    #[test]
    fn test_parse_no_dns() {
        let config = parse_with_prefix("");
        assert!(config.dns.is_none());
    }

    #[test]
    fn test_parse_inter_vlan_rules() {
        let config = parse_with_prefix(r#"
vlan "trusted" { id = 10 }
vlan "lab" {
  id = 40
  allow_from "trusted" {
    tcp = ["10.99.40.5:80", "10.99.10.50:10.99.40.5:443"]
    udp = ["10.99.40.5:53"]
  }
}
"#);
        let from_trusted = config.vlan.get("lab").unwrap().allow_from.get("trusted").unwrap();
        assert_eq!(from_trusted.tcp.len(), 2);
        assert_eq!(from_trusted.udp, vec!["10.99.40.5:53"]);
    }

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

    #[test]
    fn test_parse_full_example() {
        let input = include_str!("../examples/vlan_router.hcl");
        let config = parse_hcl(input).unwrap();
        assert_eq!(config.vlan.len(), 4);
        assert_eq!(config.wan.tcp_forward.len(), 2);
        assert!(config.qos.is_none());

        // Switch config
        let sw = config.switch.as_ref().expect("missing switch block");
        assert_eq!(sw.url, "http://192.168.2.1");
        assert_eq!(sw.user.as_deref(), Some("admin"));
        assert_eq!(sw.router_ip.as_deref(), Some("192.168.2.2/24"));
        assert_eq!(sw.port.len(), 9);
        assert_eq!(sw.port["1"].pvid, 10);
        assert_eq!(sw.port["1"].accept, "untag-only");
        assert_eq!(sw.port["1"].vlans.get("vlan_10").unwrap(), "U");
        assert_eq!(sw.port["9"].pvid, 1);
        assert_eq!(sw.port["9"].accept, "all");
        assert_eq!(sw.port["9"].vlans.len(), 5);
        assert_eq!(sw.port["9"].vlans.get("vlan_10").unwrap(), "T");
    }

    #[test]
    fn test_parse_switch_config() {
        let config = parse_with_prefix(r#"
switch {
  url        = "http://10.0.0.1"
  user       = "admin"
  pass       = "secret"
  mgmt_iface = "trunk"
  router_ip  = "10.0.0.2/24"
  port "1" {
    pvid   = 10
    accept = "untag-only"
    vlans  = { vlan_10 = "U" }
  }
  port "2" {
    pvid   = 20
    accept = "untag-only"
    vlans  = { vlan_20 = "U" }
  }
  port "3" {
    pvid   = 1
    accept = "all"
    vlans  = { vlan_10 = "T", vlan_20 = "T" }
  }
}
"#);
        let sw = config.switch.unwrap();
        assert_eq!(sw.url, "http://10.0.0.1");
        assert_eq!(sw.pass.as_deref(), Some("secret"));
        assert_eq!(sw.port.len(), 3);
        assert_eq!(sw.port["1"].pvid, 10);
        assert_eq!(sw.port["1"].vlans.get("vlan_10").unwrap(), "U");
        assert_eq!(sw.port["3"].accept, "all");
        assert_eq!(sw.port["3"].vlans.len(), 2);
    }

    #[test]
    fn test_no_switch_is_ok() {
        let config = parse_with_prefix("");
        assert!(config.switch.is_none());
    }

    #[test]
    fn test_parse_vlan_qos_class() {
        let config = parse_with_prefix(r#"
vlan "trusted" {
  id        = 10
  qos_class = "voice"
}
vlan "iot" {
  id        = 20
  qos_class = "bulk"
}
"#);
        assert_eq!(config.vlan.get("trusted").unwrap().qos_class.as_deref(), Some("voice"));
        assert_eq!(config.vlan.get("iot").unwrap().qos_class.as_deref(), Some("bulk"));
    }

    #[test]
    fn test_parse_iperf() {
        let config = parse_with_prefix(r#"
vlan "trusted" {
  id            = 10
  iperf_enabled = true
}
"#);
        assert!(config.vlan.get("trusted").unwrap().iperf_enabled);
    }

    #[test]
    fn test_parse_invalid_hcl() {
        assert!(parse_hcl("this is not valid {{{").is_err());
    }

    #[test]
    fn test_parse_missing_interfaces() {
        assert!(parse_hcl("wan {}").is_err());
    }

    #[test]
    fn test_wan_defaults() {
        let config = parse_with_prefix("");
        assert!(config.wan.enable_ipv4);
        assert!(!config.wan.enable_ipv6);
        assert!(config.wan.icmp_accept.is_empty());
    }

    #[test]
    fn test_vlan_aware_switch() {
        let config = parse_with_prefix(r#"
vlan_aware_switch = true
vlan "trusted" { id = 10 }
"#);
        assert!(config.vlan_aware_switch);
    }

    #[test]
    fn test_iperf_port() {
        let config = parse_with_prefix("iperf_port = 9999");
        assert_eq!(config.iperf_port, Some(9999));
    }

    #[test]
    fn test_vlan_forward_routes() {
        let config = parse_with_prefix(r#"
vlan "trusted" {
  id = 10
  tcp_forward = ["8080:10.99.10.50:80"]
  udp_forward = ["5353:10.99.10.50:53"]
}
"#);
        let v = config.vlan.get("trusted").unwrap();
        assert_eq!(v.tcp_forward, vec!["8080:10.99.10.50:80"]);
        assert_eq!(v.udp_forward, vec!["5353:10.99.10.50:53"]);
    }
}
