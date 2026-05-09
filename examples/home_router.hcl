# Simple home router — no managed switch, VLAN 1 on bare trunk.
# Load via: nifty-filter nftables --config home_router.hcl

interfaces {
  trunk = "trunk"
  wan   = "wan"
}

wan {
  enable_ipv4 = true

  icmp_accept = []
  tcp_accept  = []
  udp_accept  = []

  tcp_forward = []
  udp_forward = []
  # Example: forward WAN port 8080 to internal host
  # tcp_forward = ["8080:192.168.10.50:80", "2222:192.168.10.10:22"]
}

dns {
  upstream = ["1.1.1.1", "1.0.0.1"]
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

  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22, 80, 443]
    udp_accept  = [53, 67, 68]
  }

  dhcp {
    pool_start = "192.168.10.100"
    pool_end   = "192.168.10.250"
    router     = "192.168.10.1"
    dns        = "192.168.10.1"
  }

  # qos_class = "besteffort"
}
