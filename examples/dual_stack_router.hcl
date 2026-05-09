# Dual-stack (IPv4 + IPv6) router configuration.
# Load via: nifty-filter nftables --config dual_stack_router.hcl

interfaces {
  trunk = "trunk"
  wan   = "wan"
}

wan {
  enable_ipv4 = true
  enable_ipv6 = true

  # WAN port forwarding (use brackets for IPv6: port:[ipv6_addr]:port)
  tcp_forward = ["8080:192.168.10.50:80", "8443:[fd00:10::50]:443"]
  udp_forward = []
}

dns {
  upstream = ["1.1.1.1", "1.0.0.1", "2606:4700:4700::1111", "2606:4700:4700::1001"]
}

# QoS: Bufferbloat mitigation (CAKE)
# qos {
#   upload_mbps    = 20
#   download_mbps  = 300
#   shave_percent  = 10
# }

vlan "lan" {
  id = 1

  ipv4 {
    subnet = "192.168.10.1/24"
    egress = ["0.0.0.0/0"]
  }

  ipv6 {
    subnet = "fd00:10::1/64"
    egress = ["::/0"]
  }

  firewall {
    # Defaults shown — customize as needed
    # icmp_accept   = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    # icmpv6_accept = ["nd-neighbor-solicit", "nd-neighbor-advert", "nd-router-solicit", "nd-router-advert", "echo-request", "echo-reply"]
    tcp_accept = [22]
    udp_accept = [67, 68, 546, 547]
  }

  dhcp {
    pool_start = "192.168.10.100"
    pool_end   = "192.168.10.250"
    router     = "192.168.10.1"
    dns        = "192.168.10.1"
  }

  dhcpv6 {
    pool_start = "fd00:10::100"
    pool_end   = "fd00:10::1ff"
  }

  # qos_class = "besteffort"
}
