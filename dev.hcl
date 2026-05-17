# Minimal dev config for testing nftables generation.
# Usage: nifty-filter nftables --config dev.hcl

interfaces {
  trunk = "lan"
  wan   = "wan"
}

wan {
  enable_ipv4 = true
}

vlan "lan" {
  id = 1

  ipv4 {
    subnet = "192.168.10.1/24"
    egress = ["0.0.0.0/0"]
  }

  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22, 80, 443]
    udp_accept  = [67, 68]
  }
}
