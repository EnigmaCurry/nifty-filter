# nifty-filter configuration
# Edit this file, then reboot to apply changes.
#
# See examples at: https://github.com/EnigmaCurry/nifty-filter/tree/hcl/examples

# hostname = "nifty-filter"

# Interface names as seen by the OS (set by systemd-networkd .link files).
interfaces {
  trunk = "trunk"
  wan   = "wan"
}

# MAC addresses for interface renaming (used to generate .link files).
# Find your MACs with: ip link
# links {
#   wan   = "aa:bb:cc:dd:ee:01"
#   trunk = "aa:bb:cc:dd:ee:02"
# }

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
