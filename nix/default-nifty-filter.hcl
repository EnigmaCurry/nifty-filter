# nifty-filter configuration
# Edit this file, then reboot to apply changes.
#
# See examples at: https://github.com/EnigmaCurry/nifty-filter/tree/hcl/examples

# hostname = "nifty-filter"

# Interfaces:
# To give friendly names to your NICs, identify them via MAC address,
# and specify their *new* names. If the interface already has the
# name you want, don't specify the MAC address, and specify the existing name.
# Find your MACs with: ip link
interfaces {
  trunk {
    # mac  = "aa:bb:cc:dd:ee:01"
    name = "trunk"
  }
  wan {
    # mac  = "aa:bb:cc:dd:ee:02"
    name = "wan"
  }
}

# WAN-facing firewall policy. All inbound ports are closed by default.
wan {
  enable_ipv4 = true
  enable_ipv6 = false

  icmp_accept = []
  tcp_accept  = []
  udp_accept  = []
}

# DNS resolver (used by dnsmasq).
dns {
  upstream = ["1.1.1.1", "1.0.0.1"]
}

# For a VLAN-aware router, uncomment and configure:
# vlan_aware_switch = true
#
# vlan "trusted" {
#   id = 10
#   ipv4 {
#     subnet = "10.99.10.1/24"
#     egress = ["0.0.0.0/0"]
#   }
#   firewall {
#     icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
#     tcp_accept  = [22]
#     udp_accept  = [53, 67, 68]
#   }
#   dhcp {
#     pool_start = "10.99.10.100"
#     pool_end   = "10.99.10.250"
#     router     = "10.99.10.1"
#     dns        = "10.99.10.1"
#   }
# }
