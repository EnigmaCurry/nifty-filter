use crate::hcl_config::HclConfig;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Generate systemd .link files for interface renaming by MAC address.
pub fn generate_linkfiles(config: &HclConfig, output_dir: &str) -> Result<(), String> {
    let links = config
        .links
        .as_ref()
        .ok_or("No links block in config; cannot generate .link files")?;

    let dir = Path::new(output_dir);
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {}", output_dir, e))?;

    let wan_name = &config.interfaces.wan;
    let trunk_name = &config.interfaces.trunk;

    write_link_file(dir, wan_name, &links.wan)?;
    write_link_file(dir, trunk_name, &links.trunk)?;

    if let (Some(mgmt_name), Some(mgmt_mac)) = (&config.interfaces.mgmt, &links.mgmt) {
        write_link_file(dir, mgmt_name, mgmt_mac)?;
    }

    if let Some(extras) = &links.extra {
        for entry in extras {
            let (mac, name) = entry
                .split_once('=')
                .ok_or_else(|| format!("Invalid extra link entry '{}': expected MAC=name", entry))?;
            write_link_file(dir, name.trim(), mac.trim())?;
        }
    }

    Ok(())
}

fn write_link_file(dir: &Path, name: &str, mac: &str) -> Result<(), String> {
    if mac.is_empty() {
        return Err(format!("Missing MAC address for interface '{}'", name));
    }
    let path = dir.join(format!("10-{}.link", name));
    let content = format!(
        "[Match]\nMACAddress={}\n\n[Link]\nName={}\n",
        mac, name
    );
    fs::write(&path, content)
        .map_err(|e| format!("Cannot write {}: {}", path.display(), e))
}

/// Generate systemd-networkd .network and .netdev files.
pub fn generate_networkd(config: &HclConfig, output_dir: &str) -> Result<(), String> {
    let dir = Path::new(output_dir);
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {}", output_dir, e))?;

    let wan = &config.interfaces.wan;
    let trunk = &config.interfaces.trunk;

    // --- WAN (DHCP client) ---
    let mut wan_network = String::from("[Network]\n");
    if config.wan.enable_ipv4 {
        wan_network.push_str("DHCP=ipv4\n");
    }
    if config.wan.enable_ipv6 {
        wan_network.push_str("IPv6AcceptRA=yes\nIPv6Forwarding=no\n");
    }

    let wan_content = format!(
        "[Match]\nName={}\n\n{}\n[DHCPv4]\nUseDNS=yes\n\n[IPv6AcceptRA]\nUseDNS=yes\nDHCPv6Client=always\n\n[DHCPv6]\nUseDNS=no\nPrefixDelegationHint=::/60\n",
        wan, wan_network
    );
    write_file(dir, "10-wan.network", &wan_content)?;

    // --- Trunk + VLANs ---
    // Sort VLANs by ID for deterministic output
    let mut vlans_sorted: Vec<_> = config.vlan.iter().collect();
    vlans_sorted.sort_by_key(|(_, v)| v.id);

    if config.vlan_aware_switch {
        // VLAN-aware mode: trunk carries no IP, VLAN subinterfaces get addresses
        let mut trunk_vlan_lines = String::new();

        for (name, vlan) in &vlans_sorted {
            let iface = name.to_string();
            let vid = vlan.id;

            // .netdev for this VLAN
            let netdev = format!(
                "[NetDev]\nName={}\nKind=vlan\n\n[VLAN]\nId={}\n",
                iface, vid
            );
            write_file(dir, &format!("20-{}.netdev", iface), &netdev)?;

            // .network for this VLAN
            let mut vlan_net = String::from("[Match]\n");
            vlan_net.push_str(&format!("Name={}\n\n[Network]\n", iface));

            if let Some(ipv4) = &vlan.ipv4 {
                if config.wan.enable_ipv4 {
                    vlan_net.push_str(&format!("Address={}\n", ipv4.subnet));
                }
            }
            if let Some(ipv6) = &vlan.ipv6 {
                vlan_net.push_str(&format!("Address={}\nIPv6SendRA=yes\n", ipv6.subnet));
            }

            // IPv6 RA settings
            if vlan.ipv6.is_some() {
                let has_dhcpv6 = vlan.dhcpv6.is_some();
                let (managed, other, autonomous) = if has_dhcpv6 {
                    ("yes", "yes", "no")
                } else {
                    ("no", "no", "yes")
                };
                vlan_net.push_str(&format!(
                    "\n[IPv6SendRA]\nManaged={}\nOtherInformation={}\n\n[IPv6Prefix]\nPrefix={}\nAutonomous={}\n",
                    managed, other,
                    vlan.ipv6.as_ref().unwrap().subnet,
                    autonomous
                ));
            }

            write_file(dir, &format!("20-{}.network", iface), &vlan_net)?;

            trunk_vlan_lines.push_str(&format!("VLAN={}\n", iface));
        }

        // Trunk .network: no address, just VLAN membership
        let trunk_content = format!(
            "[Match]\nName={}\n\n[Link]\nRequiredForOnline=no\n\n[Network]\n{}\n",
            trunk, trunk_vlan_lines
        );
        write_file(dir, "10-trunk.network", &trunk_content)?;
    } else {
        // Simple mode (no VLANs on trunk): trunk gets the LAN IP directly
        // Use VLAN 1's subnets if available
        let vlan1 = config.vlan.values().find(|v| v.id == 1);
        let mut trunk_net = format!("[Match]\nName={}\n\n[Network]\n", trunk);

        if let Some(v1) = vlan1 {
            if let Some(ipv4) = &v1.ipv4 {
                if config.wan.enable_ipv4 {
                    trunk_net.push_str(&format!("Address={}\n", ipv4.subnet));
                }
            }
            if let Some(ipv6) = &v1.ipv6 {
                trunk_net.push_str(&format!("Address={}\nIPv6SendRA=yes\n", ipv6.subnet));
            }

            // IPv6 RA for simple mode
            if let Some(ipv6) = &v1.ipv6 {
                let has_dhcpv6 = v1.dhcpv6.is_some();
                let (managed, other, autonomous) = if has_dhcpv6 {
                    ("yes", "yes", "no")
                } else {
                    ("no", "no", "yes")
                };
                trunk_net.push_str(&format!(
                    "\n[IPv6SendRA]\nManaged={}\nOtherInformation={}\n\n[IPv6Prefix]\nPrefix={}\nAutonomous={}\n",
                    managed, other, ipv6.subnet, autonomous
                ));
            }
        }

        write_file(dir, "10-trunk.network", &trunk_net)?;
    }

    // Optional management interface
    if let (Some(mgmt_name), Some(mgmt_subnet)) =
        (&config.interfaces.mgmt, &config.interfaces.mgmt_subnet)
    {
        let mgmt_content = format!(
            "[Match]\nName={}\n\n[Network]\nAddress={}\nLinkLocalAddressing=no\nIPv6AcceptRA=no\n",
            mgmt_name, mgmt_subnet
        );
        write_file(dir, "10-mgmt.network", &mgmt_content)?;
    }

    Ok(())
}

/// Generate dnsmasq.conf from HCL configuration.
pub fn generate_dnsmasq(config: &HclConfig, output: &str) -> Result<(), String> {
    let path = Path::new(output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {}", parent.display(), e))?;
    }

    let mut out = fs::File::create(path)
        .map_err(|e| format!("Cannot create {}: {}", output, e))?;

    // Upstream DNS servers
    let dns_servers: Vec<&str> = config
        .dns
        .as_ref()
        .map(|d| d.upstream.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| vec!["1.1.1.1", "1.0.0.1"]);

    writeln!(out, "# Generated from nifty-filter HCL config").ok();
    writeln!(out, "pid-file=/run/dnsmasq.pid").ok();
    writeln!(out).ok();
    writeln!(out, "# DNS").ok();
    writeln!(out, "no-resolv").ok();
    for dns in &dns_servers {
        writeln!(out, "server={}", dns).ok();
    }
    writeln!(out, "domain-needed").ok();
    writeln!(out, "bogus-priv").ok();
    writeln!(out, "cache-size=1000").ok();
    writeln!(out).ok();
    writeln!(out, "# Localhost listeners").ok();
    writeln!(out, "listen-address=::1").ok();
    writeln!(out, "listen-address=127.0.0.1").ok();
    writeln!(out, "bind-dynamic").ok();
    writeln!(out).ok();
    writeln!(out, "dhcp-leasefile=/var/lib/dnsmasq/dnsmasq.leases").ok();
    writeln!(out, "log-dhcp").ok();

    // Sort VLANs by ID for deterministic output
    let mut vlans_sorted: Vec<_> = config.vlan.iter().collect();
    vlans_sorted.sort_by_key(|(_, v)| v.id);

    let trunk = &config.interfaces.trunk;

    for (name, vlan) in &vlans_sorted {
        let vid = vlan.id;

        // Determine interface name
        let iface = if config.vlan_aware_switch {
            name.to_string()
        } else if vid == 1 {
            trunk.clone()
        } else {
            format!("{}.{}", trunk, vid)
        };

        writeln!(out).ok();
        writeln!(out, "# VLAN {} ({})", vid, iface).ok();
        writeln!(out, "interface={}", iface).ok();

        // DHCPv4
        if let Some(dhcp) = &vlan.dhcp {
            writeln!(out, "listen-address={}", dhcp.router).ok();
            writeln!(
                out,
                "dhcp-range=interface:{},{},{},24h",
                iface, dhcp.pool_start, dhcp.pool_end
            )
            .ok();
            writeln!(
                out,
                "dhcp-option=interface:{},option:router,{}",
                iface, dhcp.router
            )
            .ok();
            writeln!(
                out,
                "dhcp-option=interface:{},option:dns-server,{}",
                iface, dhcp.dns
            )
            .ok();

            // Static hosts
            for host in &dhcp.host {
                if let Some(hostname) = &host.hostname {
                    writeln!(out, "dhcp-host={},{},{}", host.mac, host.ip, hostname).ok();
                } else {
                    writeln!(out, "dhcp-host={},{}", host.mac, host.ip).ok();
                }
            }
        }

        // DHCPv6
        if let Some(dhcpv6) = &vlan.dhcpv6 {
            if let Some(ipv6) = &vlan.ipv6 {
                let router_v6 = ipv6.subnet.split('/').next().unwrap_or("");
                // Use DHCP DNS if set, otherwise router's IPv6
                let dns_v6 = vlan
                    .dhcp
                    .as_ref()
                    .map(|d| d.dns.as_str())
                    .unwrap_or(router_v6);
                // For DHCPv6, use the router's IPv6 address as DNS if no explicit DNS
                let dns_v6_addr = if dns_v6.contains(':') {
                    dns_v6.to_string()
                } else {
                    router_v6.to_string()
                };
                writeln!(
                    out,
                    "dhcp-range=interface:{},{},{},64,24h",
                    iface, dhcpv6.pool_start, dhcpv6.pool_end
                )
                .ok();
                writeln!(
                    out,
                    "dhcp-option=interface:{},option6:dns-server,[{}]",
                    iface, dns_v6_addr
                )
                .ok();
                writeln!(out, "enable-ra").ok();
                writeln!(out, "ra-param={},60,600", iface).ok();
            }
        }
    }

    Ok(())
}

/// Generate a minimal DNS-only dnsmasq.conf (when no HCL config exists).
pub fn generate_dnsmasq_minimal(output: &str) -> Result<(), String> {
    let content = "pid-file=/run/dnsmasq.pid\nlisten-address=127.0.0.1\nbind-interfaces\nno-resolv\nserver=1.1.1.1\nserver=1.0.0.1\n";
    fs::write(output, content).map_err(|e| format!("Cannot write {}: {}", output, e))
}

fn write_file(dir: &Path, name: &str, content: &str) -> Result<(), String> {
    let path = dir.join(name);
    fs::write(&path, content).map_err(|e| format!("Cannot write {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hcl_config::parse_hcl;
    use std::fs;
    use tempfile::TempDir;

    fn parse_test_config(hcl: &str) -> HclConfig {
        parse_hcl(hcl).unwrap()
    }

    #[test]
    fn test_generate_linkfiles() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
links {
  wan   = "aa:bb:cc:dd:ee:01"
  trunk = "aa:bb:cc:dd:ee:02"
}
wan {}
"#);
        let dir = TempDir::new().unwrap();
        generate_linkfiles(&config, dir.path().to_str().unwrap()).unwrap();

        let wan_link = fs::read_to_string(dir.path().join("10-wan.link")).unwrap();
        assert!(wan_link.contains("MACAddress=aa:bb:cc:dd:ee:01"));
        assert!(wan_link.contains("Name=wan"));

        let trunk_link = fs::read_to_string(dir.path().join("10-trunk.link")).unwrap();
        assert!(trunk_link.contains("MACAddress=aa:bb:cc:dd:ee:02"));
        assert!(trunk_link.contains("Name=trunk"));
    }

    #[test]
    fn test_generate_linkfiles_with_mgmt() {
        let config = parse_test_config(r#"
interfaces {
  trunk       = "trunk"
  wan         = "wan"
  mgmt        = "mgmt0"
  mgmt_subnet = "192.168.0.1/24"
}
links {
  wan   = "aa:bb:cc:dd:ee:01"
  trunk = "aa:bb:cc:dd:ee:02"
  mgmt  = "aa:bb:cc:dd:ee:03"
}
wan {}
"#);
        let dir = TempDir::new().unwrap();
        generate_linkfiles(&config, dir.path().to_str().unwrap()).unwrap();
        assert!(dir.path().join("10-mgmt0.link").exists());
    }

    #[test]
    fn test_generate_linkfiles_with_extra() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
links {
  wan   = "aa:bb:cc:dd:ee:01"
  trunk = "aa:bb:cc:dd:ee:02"
  extra = ["ff:ff:ff:ff:ff:01=extra0"]
}
wan {}
"#);
        let dir = TempDir::new().unwrap();
        generate_linkfiles(&config, dir.path().to_str().unwrap()).unwrap();
        let extra = fs::read_to_string(dir.path().join("10-extra0.link")).unwrap();
        assert!(extra.contains("MACAddress=ff:ff:ff:ff:ff:01"));
    }

    #[test]
    fn test_generate_linkfiles_no_links_block() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {}
"#);
        let dir = TempDir::new().unwrap();
        let result = generate_linkfiles(&config, dir.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_networkd_vlan_aware() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {
  enable_ipv4 = true
  enable_ipv6 = true
}
vlan_aware_switch = true
vlan "trusted" {
  id = 10
  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"]
  }
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
  dhcpv6 {
    pool_start = "fd00:40::100"
    pool_end   = "fd00:40::1ff"
  }
}
"#);
        let dir = TempDir::new().unwrap();
        generate_networkd(&config, dir.path().to_str().unwrap()).unwrap();

        // WAN
        let wan = fs::read_to_string(dir.path().join("10-wan.network")).unwrap();
        assert!(wan.contains("DHCP=ipv4"));
        assert!(wan.contains("IPv6AcceptRA=yes"));

        // Trunk (VLAN-aware: no address, lists VLANs)
        let trunk = fs::read_to_string(dir.path().join("10-trunk.network")).unwrap();
        assert!(trunk.contains("VLAN=trusted"));
        assert!(trunk.contains("VLAN=lab"));
        assert!(!trunk.contains("Address="));

        // VLAN netdev
        let trusted_netdev = fs::read_to_string(dir.path().join("20-trusted.netdev")).unwrap();
        assert!(trusted_netdev.contains("Kind=vlan"));
        assert!(trusted_netdev.contains("Id=10"));

        // VLAN network
        let trusted_net = fs::read_to_string(dir.path().join("20-trusted.network")).unwrap();
        assert!(trusted_net.contains("Address=10.99.10.1/24"));

        // Lab with IPv6 + DHCPv6
        let lab_net = fs::read_to_string(dir.path().join("20-lab.network")).unwrap();
        assert!(lab_net.contains("Address=fd00:40::1/64"));
        assert!(lab_net.contains("IPv6SendRA=yes"));
        assert!(lab_net.contains("Managed=yes")); // DHCPv6 enabled
    }

    #[test]
    fn test_generate_networkd_simple_mode() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan { enable_ipv4 = true }
vlan "default" {
  id = 1
  ipv4 {
    subnet = "10.99.1.1/24"
    egress = ["0.0.0.0/0"]
  }
}
"#);
        let dir = TempDir::new().unwrap();
        generate_networkd(&config, dir.path().to_str().unwrap()).unwrap();

        let trunk = fs::read_to_string(dir.path().join("10-trunk.network")).unwrap();
        assert!(trunk.contains("Address=10.99.1.1/24"));
        // No .netdev in simple mode
        assert!(!dir.path().join("20-default.netdev").exists());
    }

    #[test]
    fn test_generate_networkd_mgmt() {
        let config = parse_test_config(r#"
interfaces {
  trunk       = "trunk"
  wan         = "wan"
  mgmt        = "mgmt0"
  mgmt_subnet = "192.168.88.1/24"
}
wan {}
"#);
        let dir = TempDir::new().unwrap();
        generate_networkd(&config, dir.path().to_str().unwrap()).unwrap();

        let mgmt = fs::read_to_string(dir.path().join("10-mgmt.network")).unwrap();
        assert!(mgmt.contains("Address=192.168.88.1/24"));
        assert!(mgmt.contains("LinkLocalAddressing=no"));
    }

    #[test]
    fn test_generate_dnsmasq_basic() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {}
dns { upstream = ["8.8.8.8", "8.8.4.4"] }
vlan_aware_switch = true
vlan "trusted" {
  id = 10
  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"]
  }
  dhcp {
    pool_start = "10.99.10.100"
    pool_end   = "10.99.10.250"
    router     = "10.99.10.1"
    dns        = "10.99.10.1"
  }
}
"#);
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("dnsmasq.conf");
        generate_dnsmasq(&config, output.to_str().unwrap()).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("server=8.8.8.8"));
        assert!(content.contains("server=8.8.4.4"));
        assert!(content.contains("interface=trusted"));
        assert!(content.contains("listen-address=10.99.10.1"));
        assert!(content.contains("dhcp-range=interface:trusted,10.99.10.100,10.99.10.250,24h"));
        assert!(content.contains("dhcp-option=interface:trusted,option:router,10.99.10.1"));
    }

    #[test]
    fn test_generate_dnsmasq_with_hosts() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan {}
vlan_aware_switch = true
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
      mac = "aa:bb:cc:dd:ee:02"
      ip  = "10.99.10.11"
    }
  }
}
"#);
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("dnsmasq.conf");
        generate_dnsmasq(&config, output.to_str().unwrap()).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("dhcp-host=aa:bb:cc:dd:ee:01,10.99.10.10,server1"));
        assert!(content.contains("dhcp-host=aa:bb:cc:dd:ee:02,10.99.10.11"));
    }

    #[test]
    fn test_generate_dnsmasq_dhcpv6() {
        let config = parse_test_config(r#"
interfaces {
  trunk = "trunk"
  wan   = "wan"
}
wan { enable_ipv6 = true }
vlan_aware_switch = true
vlan "lab" {
  id = 40
  ipv6 {
    subnet = "fd00:40::1/64"
    egress = ["::/0"]
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
"#);
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("dnsmasq.conf");
        generate_dnsmasq(&config, output.to_str().unwrap()).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("dhcp-range=interface:lab,fd00:40::100,fd00:40::1ff,64,24h"));
        assert!(content.contains("enable-ra"));
        assert!(content.contains("ra-param=lab,60,600"));
    }

    #[test]
    fn test_generate_dnsmasq_minimal() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("dnsmasq.conf");
        generate_dnsmasq_minimal(output.to_str().unwrap()).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("listen-address=127.0.0.1"));
        assert!(content.contains("server=1.1.1.1"));
    }
}
