# Example: VLAN-aware router with managed switch
# Four VLANs: trusted (10), iot (20), guest (30), lab (40)
#
# Load via: nifty-filter nftables --config vlan_router.hcl

interfaces {
  trunk = "trunk"
  wan   = "wan"
}

wan {
  enable_ipv4 = true
  enable_ipv6 = true

  icmp_accept = []
  tcp_accept  = []
  udp_accept  = []

  tcp_forward = [
    "443:10.99.40.50:443",
    "22:10.99.40.10:22",
  ]
  udp_forward = []
}

dns {
  upstream = ["1.1.1.1", "1.0.0.1"]
}

# --- VLAN 10: Trusted ---
# Full internet access + SSH to router
vlan "trusted" {
  id = 10

  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"]
  }

  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22]
    udp_accept  = [53, 67, 68]
  }

  dhcp {
    pool_start = "10.99.10.100"
    pool_end   = "10.99.10.250"
    router     = "10.99.10.1"
    dns        = "10.99.10.1"

    # host {
    #   mac      = "aa:bb:cc:dd:ee:01"
    #   ip       = "10.99.10.10"
    #   hostname = "server1"
    # }
    # host {
    #   mac      = "aa:bb:cc:dd:ee:02"
    #   ip       = "10.99.10.11"
    #   hostname = "nas"
    # }
  }
}

# --- VLAN 20: IoT Jail ---
# DHCP only, no internet, no router access beyond DHCP
vlan "iot" {
  id = 20

  ipv4 {
    subnet = "10.99.20.1/24"
    egress = []
  }

  firewall {
    icmp_accept = ["destination-unreachable"]
    tcp_accept  = []
    udp_accept  = [53, 67, 68]
  }

  dhcp {
    pool_start = "10.99.20.100"
    pool_end   = "10.99.20.250"
    router     = "10.99.20.1"
    dns        = "10.99.20.1"
  }
}

# --- VLAN 30: Guest ---
# Internet access but no SSH to router
vlan "guest" {
  id = 30

  ipv4 {
    subnet = "10.99.30.1/24"
    egress = ["0.0.0.0/0"]
  }

  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = []
    udp_accept  = [53, 67, 68]
  }

  dhcp {
    pool_start = "10.99.30.100"
    pool_end   = "10.99.30.250"
    router     = "10.99.30.1"
    dns        = "10.99.30.1"
  }
}

# --- VLAN 40: Lab (dual-stack) ---
# Full internet on both IPv4 and IPv6
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
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22]
    udp_accept  = [53, 67, 68, 546, 547]
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

  allow_inbound_tcp = [
    "443:[2001:db8:abcd:40::50]",
    "22:[2001:db8:abcd:40::10]",
  ]
}

# --- QoS: Bufferbloat mitigation (CAKE) ---
# qos {
#   upload_mbps    = 20
#   download_mbps  = 300
#   shave_percent  = 10
#
#   overrides {
#     voice = ["10.99.10.50", "10.99.10.51"]
#     bulk  = ["10.99.20.0/24"]
#   }
# }

# --- Managed switch (Sodola) ---
# Supervise the managed switch: enforce VLAN port assignments.
# The NixOS module extracts these settings as env vars for sodola-switch.
switch {
  url        = "http://192.168.2.1"
  user       = "admin"
  pass       = "admin"
  mgmt_iface = "trunk"
  router_ip  = "192.168.2.2/24"

  # Per-port configuration (Sodola SL-SWTGW218AS: ports 1-8 RJ45, port 9 SFP+)
  # VLANs not listed on a port are not members of that port.
  port "1" {
    pvid   = 10
    accept = "untagged-only"
    vlans { untagged = [10] }
  }
  port "2" {
    pvid   = 20
    accept = "untagged-only"
    vlans { untagged = [20] }
  }
  port "3" {
    pvid   = 30
    accept = "untagged-only"
    vlans { untagged = [30] }
  }
  port "4" {
    pvid   = 30
    accept = "untagged-only"
    vlans { untagged = [30] }
  }
  port "5" {
    pvid   = 40
    accept = "untagged-only"
    vlans { untagged = [40] }
  }
  port "6" {
    pvid   = 40
    accept = "untagged-only"
    vlans { untagged = [40] }
  }
  port "7" {
    pvid   = 40
    accept = "untagged-only"
    vlans { untagged = [40] }
  }
  port "8" {
    pvid   = 1
    accept = "all"
    label  = "management"
    vlans { untagged = [1] }
  }
  port "9" {
    pvid   = 1
    accept = "all"
    label  = "trunk/uplink"
    vlans {
      untagged = [1]
      tagged   = [10, 20, 30, 40]
    }
  }
}
