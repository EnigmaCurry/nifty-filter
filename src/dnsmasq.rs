use crate::parsers::*;
use askama::Template;
use std::net::IpAddr;

#[derive(Template)]
#[template(path = "dnsmasq.conf.txt")]
pub struct DnsmasqTemplate {
    dns_upstream_1: IpAddr,
    dns_upstream_2: IpAddr,
    domain_lan: domain::Domain,
    interface: Interface,
    listen_address: IpAddr,
    dhcp_lan_range_start: IpAddr,
    dhcp_lan_range_end: IpAddr,
    dhcp_lan_lease_time: dhcp_lease_time::DHCPLeaseTime,
    gateway_lan: IpAddr,
    dns_lan: IpAddr,
}

impl DnsmasqTemplate {
    pub fn from_env() -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let interface = get_interface("INTERFACE", &mut errors);
        let listen_address = get_ip_address("LISTEN_ADDRESS", &mut errors);
        let dns_upstream_1 = get_ip_address("DNS_UPSTREAM_1", &mut errors);
        let dns_upstream_2 = get_ip_address("DNS_UPSTREAM_2", &mut errors);
        let domain_lan = get_domain_name("DOMAIN_LAN", &mut errors);
        let dhcp_lan_range_start = get_ip_address("DHCP_LAN_RANGE_START", &mut errors);
        let dhcp_lan_range_end = get_ip_address("DHCP_LAN_RANGE_END", &mut errors);
        let dhcp_lan_lease = get_dhcp_lease_time("DHCP_LAN_LEASE", &mut errors);
        let gateway_lan = get_ip_address("GATEWAY_LAN", &mut errors);
        let dns_lan = get_ip_address("DNS_LAN", &mut errors);

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(DnsmasqTemplate {
            interface,
            listen_address,
            dns_upstream_1,
            dns_upstream_2,
            domain_lan,
            dhcp_lan_range_start,
            dhcp_lan_range_end,
            dhcp_lan_lease_time: dhcp_lan_lease,
            gateway_lan,
            dns_lan,
        })
    }
}
