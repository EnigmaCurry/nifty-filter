# Example: VLAN-aware router with managed switch
# Four VLANs: trusted (10), iot (20), guest (30), lab (40)
# Load via: nifty-filter nftables --config vlan_router.hcl

# Your router machine must have at least two network interfaces (NICs):
# 1) trunk:
##   trunk is the backend (LAN side) interface trafficking ALL VLANs.
##   trunk MUST be connected to a managed switch.
##     (if you don't have one, use home_router.hcl instead).
##   If your switch has mixed speed ports, plug trunk into the fastest one.
# 2) wan:
##   wan is the upstream (internet) connection.
# 3) mgmt (optional):
##   mgmt is the optional management interface, for direct access to
##   the router for configuration, upgrade, and maintainance purposes.

vlan_aware_switch = true

# Interfaces:
## The nifty-filter config uses logical names for interfaces: trunk, wan, mgmt.
## However, your physical NICs may be named differently.
## To give friendly names to your NICs, identify them via their MAC address,
## and specify their *new* names. It is recommended that you stick
## with the names "trunk", "wan", "mgmt". If the interfaces already have
## the names you want, you don't need to specify the MAC addresses,
## and you only need to specify their existing name.
interfaces {
  trunk {
    mac = "aa:bb:cc:dd:ee:01" # the real MAC address of the LAN-side interface.
    name = "trunk"            # the new name you want for the interface.
  }
  wan {
    mac = "aa:bb:cc:dd:ee:02" # the real MAC address of the WAN-side interface.
    name = "wan"              # the new name you want for the interface.
  }
  mgmt {
    mac = "aa:bb:cc:dd:ee:03" # the real MAC address of the MGMT interface.
    name = "mgmt"             # the new name you want for the interface.
    subnet = "10.99.0.1/24"   # Static subnet for the MGMT interface only.
  }
}

# WAN-facing firewall policy:
##   All inbound ports are closed by default.
##   Uncomment tcp_forward/udp_forward to expose services via DNAT.
wan {
  enable_ipv4 = true
  enable_ipv6 = true

  icmp_accept = []  # e.g. ["echo-request"] to allow ping
  tcp_accept  = []  # e.g. [22] to allow SSH
  udp_accept  = []  # e.g. [51820, 1194] to allow WireGuard and OpenVPN

  # Port forwarding (DNAT) to internal hosts: "wan_port:dest_ip:dest_port"
  #tcp_forward = [
  #  "443:10.99.40.50:443",
  #  "22:10.99.40.10:22",
  #]
  #udp_forward = [
  #  "51820:10.99.40.50:51820",
  #]
}

# DNS resolver forwarded to upstream servers (used by dnsmasq, not VLAN clients).
dns {
  upstream = ["1.1.1.1", "1.0.0.1"]
}

# --- VLAN 10: Trusted ---
# Full internet access + SSH to router
vlan "trusted" {
  id = 10

  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"] # Full outbound access to the internet
  }

  # Firewall rules for VLAN 10, specifically, what can clients connect to on the router itself?
  #   Default example allows these connections made from VLAN clients:
  #    - ["echo-request", ...] pinging the router.
  #    - [22] SSH into the router.
  #    - [53] DNS requests made to the router IP.
  #    - [67, 68] DHCP requests handled by the router.
  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
    tcp_accept  = [22]
    udp_accept  = [53, 67, 68]
  }

  # DHCP config for dnsmasq - clients receive IP addresses from the range defined.
  ## You may add static hosts by uncommenting the host subsection.
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
# bandwidth limits hard-cap this VLAN's WAN egress (upload), non-burstable.
# Requires the qos block to be enabled.
vlan "iot" {
  id = 20

  # bandwidth {
  #   upload_mbps = 5
  # }

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
## You SHOULD uncomment the `qos` section, but you need to find out
## what your real maximum upload/download bandwidth of your WAN
## connection is before doing so. Run iperf or use speedtest.net. Set
## the upload_mbps and download_mbps below according to your actual
## (peak) results. The shave_percent will throttle your connection
## below the actual limit you set, so that the bottleneck remains on
## your router rather than your ISP. This bottleneck will reduce your
## peak transfer rate, but will give the router more headroom to
## effectively prioritize traffic.

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
## nifty-filter has OPTIONAL support to manage your Sodola switch for
## you, uncomment the config and set the port assignments to the VLANs you desire.
#
# Supervise the managed switch: enforce VLAN port assignments.
# The NixOS module extracts these settings as env vars for sodola-switch.
# switch {
#   url        = "http://192.168.2.1"
#   user       = "admin"
#   pass       = "admin"
#   mgmt_iface = "trunk"
#   router_ip  = "192.168.2.2/24"
#
#   # Per-port configuration (Sodola SL-SWTGW218AS: ports 1-8 RJ45, port 9 SFP+)
#   # VLANs not listed on a port are not members of that port.
#   port "1" {
#     pvid   = 10
#     accept = "untagged-only"
#     vlans { untagged = [10] }
#   }
#   port "2" {
#     pvid   = 20
#     accept = "untagged-only"
#     vlans { untagged = [20] }
#   }
#   port "3" {
#     pvid   = 30
#     accept = "untagged-only"
#     vlans { untagged = [30] }
#   }
#   port "4" {
#     pvid   = 30
#     accept = "untagged-only"
#     vlans { untagged = [30] }
#   }
#   port "5" {
#     pvid   = 40
#     accept = "untagged-only"
#     vlans { untagged = [40] }
#   }
#   port "6" {
#     pvid   = 40
#     accept = "untagged-only"
#     vlans { untagged = [40] }
#   }
#   port "7" {
#     pvid   = 40
#     accept = "tagged-only"
#     label  = "secondary switch"
#     vlans {
#       tagged = [20, 40]
#     }
#   }
#   port "8" {
#     pvid   = 1
#     accept = "all"
#     label  = "management"
#     vlans { untagged = [1] }
#   }
#   port "9" {
#     pvid   = 1
#     accept = "all"
#     label  = "trunk/uplink"
#     vlans {
#       untagged = [1]
#       tagged   = [10, 20, 30, 40]
#     }
#   }
# }
