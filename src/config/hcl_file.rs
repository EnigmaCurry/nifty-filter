use std::fmt::Write;
use std::fs;
use std::path::Path;

use crate::hcl_config::*;

/// Load and parse an HCL config file.
pub fn load(path: &Path) -> Result<HclConfig, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    parse_hcl(&content)
}

/// Save an HclConfig to a file.
pub fn save(config: &HclConfig, path: &Path) -> Result<(), String> {
    let content = format_hcl(config);
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &content).map_err(|e| format!("Cannot write {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, path).map_err(|e| {
        format!(
            "Cannot rename {} -> {}: {}",
            tmp.display(),
            path.display(),
            e
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HCL writer
// ---------------------------------------------------------------------------

struct HclWriter {
    buf: String,
    depth: usize,
}

impl HclWriter {
    fn new() -> Self {
        Self {
            buf: String::new(),
            depth: 0,
        }
    }

    fn indent(&self) -> String {
        "  ".repeat(self.depth)
    }

    fn line(&mut self, text: &str) {
        writeln!(self.buf, "{}{}", self.indent(), text).unwrap();
    }

    fn blank(&mut self) {
        self.buf.push('\n');
    }

    fn open(&mut self, name: &str) {
        self.line(&format!("{name} {{"));
        self.depth += 1;
    }

    fn open_labeled(&mut self, kind: &str, label: &str) {
        self.line(&format!("{kind} \"{label}\" {{"));
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.line("}");
    }

    fn str_attr(&mut self, key: &str, val: &str) {
        self.line(&format!("{key} = \"{val}\""));
    }

    fn bool_attr(&mut self, key: &str, val: bool) {
        self.line(&format!("{key} = {val}"));
    }

    fn num_attr<T: std::fmt::Display>(&mut self, key: &str, val: T) {
        self.line(&format!("{key} = {val}"));
    }

    fn string_array(&mut self, key: &str, vals: &[String]) {
        if vals.is_empty() {
            self.line(&format!("{key} = []"));
        } else {
            let items: Vec<String> = vals.iter().map(|v| format!("\"{}\"", v)).collect();
            let joined = items.join(", ");
            if joined.len() < 60 {
                self.line(&format!("{key} = [{joined}]"));
            } else {
                self.line(&format!("{key} = ["));
                self.depth += 1;
                for v in vals {
                    self.line(&format!("\"{v}\","));
                }
                self.depth -= 1;
                self.line("]");
            }
        }
    }

    fn u16_array(&mut self, key: &str, vals: &[u16]) {
        if vals.is_empty() {
            self.line(&format!("{key} = []"));
        } else {
            let nums: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
            self.line(&format!("{key} = [{}]", nums.join(", ")));
        }
    }

    fn into_string(self) -> String {
        self.buf
    }
}

// ---------------------------------------------------------------------------
// Format HclConfig -> HCL text
// ---------------------------------------------------------------------------

pub fn format_hcl(config: &HclConfig) -> String {
    let mut w = HclWriter::new();

    w.line("# nifty-filter configuration");
    w.blank();

    // Top-level optional attributes
    if let Some(ref hostname) = config.hostname {
        w.str_attr("hostname", hostname);
    }
    if let Some(port) = config.dashboard_port {
        w.num_attr("dashboard_port", port);
    }
    if let Some(port) = config.iperf_port {
        w.num_attr("iperf_port", port);
    }
    if config.vlan_aware_switch {
        w.bool_attr("vlan_aware_switch", true);
    }
    w.blank();

    // interfaces
    write_interfaces(&mut w, &config.interfaces);
    w.blank();

    // wan
    write_wan(&mut w, &config.wan);
    w.blank();

    // dns
    if let Some(ref dns) = config.dns {
        write_dns(&mut w, dns);
        w.blank();
    }

    // vlans (sorted by id)
    let mut vlans: Vec<(&String, &VlanHclConfig)> = config.vlan.iter().collect();
    vlans.sort_by_key(|(_, v)| v.id);
    for (name, vlan) in vlans {
        write_vlan(&mut w, name, vlan);
        w.blank();
    }

    // qos
    if let Some(ref qos) = config.qos {
        write_qos(&mut w, qos);
        w.blank();
    }

    // switch
    if let Some(ref sw) = config.switch {
        write_switch(&mut w, sw, &config.vlan);
        w.blank();
    }

    w.into_string()
}

fn write_interfaces(w: &mut HclWriter, ifaces: &InterfacesConfig) {
    w.open("interfaces");

    // trunk
    w.open("trunk");
    w.str_attr("name", &ifaces.trunk.name);
    if let Some(ref mac) = ifaces.trunk.mac {
        w.str_attr("mac", mac);
    }
    w.close();

    // wan
    w.open("wan");
    w.str_attr("name", &ifaces.wan.name);
    if let Some(ref mac) = ifaces.wan.mac {
        w.str_attr("mac", mac);
    }
    w.close();

    // mgmt
    if let Some(ref mgmt) = ifaces.mgmt {
        w.open("mgmt");
        w.str_attr("name", &mgmt.name);
        if let Some(ref mac) = mgmt.mac {
            w.str_attr("mac", mac);
        }
        if let Some(ref subnet) = mgmt.subnet {
            w.str_attr("subnet", subnet);
        }
        w.close();
    }

    w.close();
}

fn write_wan(w: &mut HclWriter, wan: &WanConfig) {
    w.open("wan");
    w.bool_attr("enable_ipv4", wan.enable_ipv4);
    w.bool_attr("enable_ipv6", wan.enable_ipv6);
    w.blank();
    w.string_array("icmp_accept", &wan.icmp_accept);
    if !wan.icmpv6_accept.is_empty() {
        w.string_array("icmpv6_accept", &wan.icmpv6_accept);
    }
    w.u16_array("tcp_accept", &wan.tcp_accept);
    w.u16_array("udp_accept", &wan.udp_accept);
    if !wan.tcp_forward.is_empty() {
        w.blank();
        w.string_array("tcp_forward", &wan.tcp_forward);
    }
    if !wan.udp_forward.is_empty() {
        if wan.tcp_forward.is_empty() {
            w.blank();
        }
        w.string_array("udp_forward", &wan.udp_forward);
    }
    w.close();
}

fn write_dns(w: &mut HclWriter, dns: &DnsConfig) {
    w.open("dns");
    w.string_array("upstream", &dns.upstream);
    w.close();
}

fn write_vlan(w: &mut HclWriter, name: &str, vlan: &VlanHclConfig) {
    w.open_labeled("vlan", name);
    w.num_attr("id", vlan.id);

    if let Some(ref bw) = vlan.bandwidth {
        w.blank();
        w.open("bandwidth");
        if let Some(up) = bw.upload_mbps {
            w.num_attr("upload_mbps", up);
        }
        if let Some(down) = bw.download_mbps {
            w.num_attr("download_mbps", down);
        }
        w.close();
    }

    if let Some(ref ipv4) = vlan.ipv4 {
        w.blank();
        w.open("ipv4");
        w.str_attr("subnet", &ipv4.subnet);
        w.string_array("egress", &ipv4.egress);
        w.close();
    }

    if let Some(ref ipv6) = vlan.ipv6 {
        w.blank();
        w.open("ipv6");
        w.str_attr("subnet", &ipv6.subnet);
        w.string_array("egress", &ipv6.egress);
        w.close();
    }

    if let Some(ref fw) = vlan.firewall {
        w.blank();
        w.open("firewall");
        w.string_array("icmp_accept", &fw.icmp_accept);
        if !fw.icmpv6_accept.is_empty() {
            w.string_array("icmpv6_accept", &fw.icmpv6_accept);
        }
        w.u16_array("tcp_accept", &fw.tcp_accept);
        w.u16_array("udp_accept", &fw.udp_accept);
        w.close();
    }

    if let Some(ref dhcp) = vlan.dhcp {
        w.blank();
        w.open("dhcp");
        w.str_attr("pool_start", &dhcp.pool_start);
        w.str_attr("pool_end", &dhcp.pool_end);
        w.str_attr("router", &dhcp.router);
        w.str_attr("dns", &dhcp.dns);
        for host in &dhcp.host {
            w.blank();
            w.open("host");
            w.str_attr("mac", &host.mac);
            w.str_attr("ip", &host.ip);
            if let Some(ref hostname) = host.hostname {
                w.str_attr("hostname", hostname);
            }
            w.close();
        }
        w.close();
    }

    if let Some(ref dhcpv6) = vlan.dhcpv6 {
        w.blank();
        w.open("dhcpv6");
        w.str_attr("pool_start", &dhcpv6.pool_start);
        w.str_attr("pool_end", &dhcpv6.pool_end);
        w.close();
    }

    if let Some(ref qos_class) = vlan.qos_class {
        w.blank();
        w.str_attr("qos_class", qos_class);
    }

    if vlan.iperf_enabled {
        w.bool_attr("iperf_enabled", true);
    }

    if !vlan.tcp_forward.is_empty() {
        w.blank();
        w.string_array("tcp_forward", &vlan.tcp_forward);
    }
    if !vlan.udp_forward.is_empty() {
        w.string_array("udp_forward", &vlan.udp_forward);
    }

    if !vlan.allow_inbound_tcp.is_empty() {
        w.blank();
        w.string_array("allow_inbound_tcp", &vlan.allow_inbound_tcp);
    }
    if !vlan.allow_inbound_udp.is_empty() {
        w.string_array("allow_inbound_udp", &vlan.allow_inbound_udp);
    }

    // inter-vlan rules
    let mut allow_from: Vec<_> = vlan.allow_from.iter().collect();
    allow_from.sort_by_key(|(name, _)| (*name).clone());
    for (from_name, rules) in allow_from {
        w.blank();
        w.open_labeled("allow_from", from_name);
        if !rules.tcp.is_empty() {
            w.string_array("tcp", &rules.tcp);
        }
        if !rules.udp.is_empty() {
            w.string_array("udp", &rules.udp);
        }
        w.close();
    }

    w.close();
}

fn write_qos(w: &mut HclWriter, qos: &QosHclConfig) {
    w.open("qos");
    w.num_attr("upload_mbps", qos.upload_mbps);
    w.num_attr("download_mbps", qos.download_mbps);
    w.num_attr("shave_percent", qos.shave_percent);
    if let Some(ref overrides) = qos.overrides {
        w.blank();
        w.open("overrides");
        if !overrides.voice.is_empty() {
            w.string_array("voice", &overrides.voice);
        }
        if !overrides.video.is_empty() {
            w.string_array("video", &overrides.video);
        }
        if !overrides.besteffort.is_empty() {
            w.string_array("besteffort", &overrides.besteffort);
        }
        if !overrides.bulk.is_empty() {
            w.string_array("bulk", &overrides.bulk);
        }
        w.close();
    }
    w.close();
}

fn write_switch(w: &mut HclWriter, sw: &SwitchConfig, vlans: &std::collections::HashMap<String, VlanHclConfig>) {
    w.open("switch");
    w.str_attr("url", &sw.url);
    if let Some(ref user) = sw.user {
        w.str_attr("user", user);
    }
    if let Some(ref pass) = sw.pass {
        w.str_attr("pass", pass);
    }
    if let Some(ref iface) = sw.mgmt_iface {
        w.str_attr("mgmt_iface", iface);
    }
    if let Some(ref ip) = sw.router_ip {
        w.str_attr("router_ip", ip);
    }

    // Ports sorted numerically
    let mut ports: Vec<(&String, &SwitchPortConfig)> = sw.port.iter().collect();
    ports.sort_by_key(|(k, _)| k.parse::<u32>().unwrap_or(u32::MAX));
    for (port_id, port) in ports {
        w.blank();
        w.open_labeled("port", port_id);
        w.num_attr("pvid", port.pvid);
        w.str_attr("accept", &port.accept);
        if let Some(ref label) = port.label {
            w.str_attr("label", label);
        } else {
            // Write a comment showing the resolved label
            let resolved = port.resolve_label_from_hcl(vlans);
            if resolved != format!("VLAN {}", port.pvid) {
                w.str_attr("label", &resolved);
            }
        }
        if let Some(ref v) = port.vlans {
            w.open("vlans");
            if !v.untagged.is_empty() {
                w.u16_array("untagged", &v.untagged);
            }
            if !v.tagged.is_empty() {
                w.u16_array("tagged", &v.tagged);
            }
            w.close();
        }
        w.close();
    }

    w.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_minimal() {
        let hcl = r#"
interfaces {
  trunk { name = "trunk" }
  wan   { name = "wan" }
}
wan {}
"#;
        let config = parse_hcl(hcl).unwrap();
        let output = format_hcl(&config);
        // Verify it parses back
        let reparsed = parse_hcl(&output).unwrap();
        assert_eq!(reparsed.interfaces.trunk_name(), "trunk");
        assert_eq!(reparsed.interfaces.wan_name(), "wan");
        assert!(reparsed.wan.enable_ipv4);
        assert!(!reparsed.wan.enable_ipv6);
    }

    #[test]
    fn round_trip_with_vlans() {
        let hcl = r#"
hostname = "test-router"
interfaces {
  trunk { name = "trunk" }
  wan   { name = "wan" }
}
wan {
  enable_ipv4 = true
  enable_ipv6 = true
  tcp_accept = [22, 80]
}
dns {
  upstream = ["1.1.1.1"]
}
vlan "trusted" {
  id = 10
  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"]
  }
  firewall {
    icmp_accept = ["echo-request"]
    tcp_accept  = [22]
    udp_accept  = [53, 67, 68]
  }
  dhcp {
    pool_start = "10.99.10.100"
    pool_end   = "10.99.10.250"
    router     = "10.99.10.1"
    dns        = "10.99.10.1"
  }
}
vlan "iot" {
  id = 20
  ipv4 {
    subnet = "10.99.20.1/24"
    egress = []
  }
}
"#;
        let config = parse_hcl(hcl).unwrap();
        let output = format_hcl(&config);
        let reparsed = parse_hcl(&output).unwrap();
        assert_eq!(reparsed.hostname.as_deref(), Some("test-router"));
        assert_eq!(reparsed.vlan.len(), 2);
        assert_eq!(reparsed.vlan.get("trusted").unwrap().id, 10);
        assert_eq!(reparsed.vlan.get("iot").unwrap().id, 20);
        let dhcp = reparsed
            .vlan
            .get("trusted")
            .unwrap()
            .dhcp
            .as_ref()
            .unwrap();
        assert_eq!(dhcp.pool_start, "10.99.10.100");
        assert_eq!(reparsed.dns.unwrap().upstream, vec!["1.1.1.1"]);
    }

    #[test]
    fn round_trip_full_example() {
        let hcl = include_str!("../../examples/vlan_router.hcl");
        let config = parse_hcl(hcl).unwrap();
        let output = format_hcl(&config);
        let reparsed = parse_hcl(&output).unwrap();
        assert_eq!(reparsed.vlan.len(), 4);
        assert!(reparsed.vlan_aware_switch);
        assert!(reparsed.wan.enable_ipv6);
        assert!(reparsed.qos.is_some());
        assert!(reparsed.switch.is_some());
    }

    #[test]
    fn round_trip_dhcp_hosts() {
        let hcl = r#"
interfaces {
  trunk { name = "trunk" }
  wan   { name = "wan" }
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
  }
}
"#;
        let config = parse_hcl(hcl).unwrap();
        let output = format_hcl(&config);
        let reparsed = parse_hcl(&output).unwrap();
        let hosts = &reparsed
            .vlan
            .get("trusted")
            .unwrap()
            .dhcp
            .as_ref()
            .unwrap()
            .host;
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(hosts[0].hostname.as_deref(), Some("server1"));
    }

    #[test]
    fn round_trip_inter_vlan() {
        let hcl = r#"
interfaces {
  trunk { name = "trunk" }
  wan   { name = "wan" }
}
wan {}
vlan "trusted" { id = 10 }
vlan "lab" {
  id = 40
  allow_from "trusted" {
    tcp = ["10.99.40.5:80"]
    udp = ["10.99.40.5:53"]
  }
}
"#;
        let config = parse_hcl(hcl).unwrap();
        let output = format_hcl(&config);
        let reparsed = parse_hcl(&output).unwrap();
        let from = reparsed
            .vlan
            .get("lab")
            .unwrap()
            .allow_from
            .get("trusted")
            .unwrap();
        assert_eq!(from.tcp, vec!["10.99.40.5:80"]);
        assert_eq!(from.udp, vec!["10.99.40.5:53"]);
    }
}
