use crate::env;
use crate::parsers::*;
use askama::Template;

#[derive(Template)]
#[template(path = "router.nft.txt")]
pub struct RouterTemplate {
    interface_mgmt: Option<Interface>,
    interface_lan: Interface,
    interface_wan: Interface,
    icmp_accept_wan: String,
    icmp_accept_lan: String,
    subnet_lan: Subnet,
    tcp_accept_lan: String,
    udp_accept_lan: String,
    tcp_accept_wan: String,
    udp_accept_wan: String,
    tcp_forward_lan: ForwardRouteList,
    udp_forward_lan: ForwardRouteList,
    tcp_forward_wan: ForwardRouteList,
    udp_forward_wan: ForwardRouteList,
}

impl RouterTemplate {
    pub fn from_env() -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        // Optional management interface
        let interface_mgmt = env::var("INTERFACE_MGMT")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|_val| get_interface("INTERFACE_MGMT", &mut errors));
        let interface_lan = get_interface("INTERFACE_LAN", &mut errors);
        let interface_wan = get_interface("INTERFACE_WAN", &mut errors);
        let subnet_lan = get_subnet("SUBNET_LAN", &mut errors);

        let icmp_accept_lan = IcmpType::vec_to_string(&get_icmp_types(
            "ICMP_ACCEPT_LAN",
            &mut errors,
            vec![
                IcmpType::EchoRequest,
                IcmpType::EchoReply,
                IcmpType::DestinationUnreachable,
                IcmpType::TimeExceeded,
            ],
        ));
        let icmp_accept_wan =
            IcmpType::vec_to_string(&get_icmp_types("ICMP_ACCEPT_WAN", &mut errors, vec![]));

        let tcp_accept_lan = get_port_accept(
            "TCP_ACCEPT_LAN",
            &mut errors,
            port::PortList::new("22,80,443").unwrap(),
        )
        .to_string();
        let udp_accept_lan = get_port_accept(
            "UDP_ACCEPT_LAN",
            &mut errors,
            port::PortList::new("").unwrap(),
        )
        .to_string();
        let tcp_accept_wan = get_port_accept(
            "TCP_ACCEPT_WAN",
            &mut errors,
            port::PortList::new("").unwrap(),
        )
        .to_string();
        let udp_accept_wan = get_port_accept(
            "UDP_ACCEPT_WAN",
            &mut errors,
            port::PortList::new("").unwrap(),
        )
        .to_string();

        let tcp_forward_lan = get_forward_routes(
            "TCP_FORWARD_LAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let udp_forward_lan = get_forward_routes(
            "UDP_FORWARD_LAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let tcp_forward_wan = get_forward_routes(
            "TCP_FORWARD_WAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );
        let udp_forward_wan = get_forward_routes(
            "UDP_FORWARD_WAN",
            &mut errors,
            ForwardRouteList::new("").unwrap(),
        );

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_mgmt,
            interface_lan,
            interface_wan,
            subnet_lan,
            icmp_accept_lan,
            icmp_accept_wan,
            tcp_accept_lan,
            udp_accept_lan,
            tcp_accept_wan,
            udp_accept_wan,
            tcp_forward_lan,
            udp_forward_lan,
            tcp_forward_wan,
            udp_forward_wan,
        })
    }
}
