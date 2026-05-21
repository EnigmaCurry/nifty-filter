# Example: VLAN-aware router with managed switch
# Four VLANs: trusted (10), iot (20), guest (30), lab (40)
# Load via: nifty-filter nftables --config vlan_router.hcl

# Enable VLAN support, which requires a *managed* switch:
vlan_aware_switch = true

# Network interfaces:
## The MAC addresses must match your actual hardware - but the names you may customize.
## 1) TRUNK interface connects to the *managed* switch, for all tagged VLAN traffic.
## 2) WAN is the upstream (internet) connection.
## 3) MGMT is your dedicated management interface.
interfaces {
  trunk {
    mac = "aa:bb:cc:dd:ee:01" # the real MAC address of the VLAN-side interface.
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

# --- QoS: Bufferbloat mitigation (CAKE) ---
## You must run a speed test (speedtest.net) and record your peak upload/download rate:
## QoS will be disabled if these rates are not set:
qos {
  #upload_mbps    = 20
  #download_mbps  = 300
  shave_percent  = 10

  # overrides {
  #   voice = ["10.99.10.50", "10.99.10.51"]
  #   bulk  = ["10.99.20.0/24"]
  # }
}

# Services configuration for the infrastructure VM (nifty-service-monitor).
# The service monitor polls /api/services-config and applies these settings.
services {
  host {
    # IP address of the infra-services VM:
    ip_address = "10.99.2.2"
    # The apex domain for your deployment:
    #  - You can use a sub-domain of a real domain you control, e.g. `my-home.example.com`.
    #  - You can use a fake domain ending in `.internal`, e.g. `nifty.internal`
    domain     = "nifty.internal"
  }

  # Traefik reverse proxy routes for the infra-services VM.
  # This limits which clients may reach each service.
  # Each route creates a Host(`<name>.<domain>`) rule in Traefik.
  # `allow_from` is required — routes without it are not exposed.
  traefik {
    route "dns" {
      ## Route to local technitium DNS web admin service:
      backend    = "http://127.0.0.1:5380"
      ## Allowed VLANs for this route:
      allow_from = ["10.99.2.0/24", "10.99.10.0/24"]
    }
    route "ddns" {
      ## Route to ddns-updater web dashboard:
      backend    = "http://127.0.0.1:8000"
      allow_from = ["10.99.2.0/24", "10.99.10.0/24"]
    }
  }

  # Local DNS is handled by a combination of dnsmasq and technitium:
  #  - Dnsmasq on the router is a non-caching, forwarding DNS resolver (and DHCP
  #    service) for all VLAN clients.
  #    (e.g. trusted=10.99.10.1, iot=10.99.20.1, etc.)
  #  - Dnsmasq forwards DNS requests to Technitium, which runs on the
  #    infra-services VM (e.g., 10.99.2.2).
  #  - VLAN clients do not require a route to the technititum DNS, because
  #    dnsmasq forwards all requests.
  #  - In this example, Technitium is configured to be a forwarding
  #    resolver using Cloudflare DNS over TLS. It can also be
  #    configured as a recursive resolver using root DNS hints.
  #  - Technitium also becomes a local authority for `nifty.internal`,
  #    or any other zones you wish to define.
  dns {
    # ---
    # Set the dnsmasq upstream DNS to include both technitium (10.99.2.2) and
    # a fallback for when technitium is unavailable/booting (1.1.1.1):
    upstream       = ["10.99.2.2", "1.1.1.1"]
    # A technitium account with limited read-only permissions is maintained for
    # the nifty-dashboard to monitor it. This password declares the current value,
    # so you can update it here and it will be automatically reapplied:
    viewer_password = "changeme"

    # Forwarders: further upstream DNS servers for Technitium to query.
    # If empty, Technitium acts as a recursive resolver using root hints.
    forwarders          = ["1.1.1.1", "1.0.0.1"]
    forwarder_protocol  = "tls"   # udp, tcp, tls, https, quic
    forwarder_concurrency = 2     # 1-10, used when multiple forwarders

    # Define all of your extra zones here
    zone "nifty.internal" {
      A = {
        "@" = "10.99.0.1" # apex domain points to the nifty-dashboard
        ddns = "10.99.2.2" # ddns-updater web dashboard on infra-services VM
        dns = "10.99.2.2" # dns domain points to technitium infra-services VM
        ntp = "10.99.2.2" # ntp points to chrony on the infra-services VM
      }
    }
  }

  # Dynamic DNS: keep external DNS records updated with the current WAN IP.
  # Each record block defines a domain to update at a given provider.
  # Provider-specific fields (token, zone_identifier, etc.) are passed through.
  # See provider docs: https://github.com/qdm12/ddns-updater#configuration
  # ddns {
  #   period = "5m"
  #   record "myhost.duckdns.org" {
  #     provider = "duckdns"
  #     token    = "your-duckdns-token"
  #   }
  # }
}
###
# --- VLAN 1: RESERVED -- DO NOT use VLAN 1 ---
###


###
# --- VLAN 2: Infrastructure ---
# Services VM (NTP, DNS, monitoring, etc.) lives here.
# Runs Technitium DNS (10.99.2.2:53) and Chrony NTP (10.99.2.2:123).
# dnsmasq on the router forwards DNS queries to Technitium.
# Uses a dedicated interface (virtual NIC on an isolated bridge) instead of
# a trunk subinterface, so the services VM is self-contained on the hypervisor.
vlan "infra" {
  id = 2

  # Dedicated interface — not on the trunk/switch.
  # The router gets a virtual NIC on the same bridge as the services VM.
  interface {
    mac  = "aa:bb:cc:dd:ee:10"
    name = "infra"
  }

  ipv4 {
    subnet = "10.99.2.1/24"
    egress = ["0.0.0.0/0"]
  }

  firewall {
    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable"]
    tcp_accept  = [22, 53, 80, 443, 3000] # 80/443 redirected to dashboard (port 3000)
    udp_accept  = [53, 67, 68]
  }

  dhcp {
    pool_start = "10.99.2.100"
    pool_end   = "10.99.2.250"
    router     = "10.99.2.1"
    dns        = "10.99.2.1"
    ntp        = "10.99.2.2"

    # host {
    #   mac      = "aa:bb:cc:dd:ee:01"
    #   ip       = "10.99.2.10"
    #   hostname = "server1"
    # }
    # host {
    #   mac      = "aa:bb:cc:dd:ee:02"
    #   ip       = "10.99.2.11"
    #   hostname = "nas"
    # }
  }

  # Allow NTP (chrony) and Traefik (HTTP/HTTPS) access from all VLANs
  allow_from "trusted" {
    tcp = ["10.99.2.2:80", "10.99.2.2:443"]
    udp = ["10.99.2.2:123"]
  }
  allow_from "iot" {
    tcp = ["10.99.2.2:80", "10.99.2.2:443"]
    udp = ["10.99.2.2:123"]
  }
  allow_from "guest" {
    tcp = ["10.99.2.2:80", "10.99.2.2:443"]
    udp = ["10.99.2.2:123"]
  }
  allow_from "lab" {
    tcp = ["10.99.2.2:80", "10.99.2.2:443"]
    udp = ["10.99.2.2:123"]
  }
}

###
# --- VLAN 10: Trusted ---
# Full internet access + SSH to router
vlan "trusted" {
  id = 10
  mdns_reflector = true  # Reflect .local mDNS to other participating VLANs

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
    tcp_accept  = [22, 80, 443, 3000] # 80/443 redirected to dashboard (port 3000)
    udp_accept  = [53, 67, 68]
  }

  # DHCP config for dnsmasq - clients receive IP addresses from the range defined.
  dhcp {
    pool_start = "10.99.10.100"
    pool_end   = "10.99.10.250"
    router     = "10.99.10.1"
    dns        = "10.99.10.1"
    ntp        = "10.99.2.2"
  }
}

###
# --- VLAN 20: IoT Jail ---
# DHCP only, no internet, no router access beyond DHCP
# bandwidth limits hard-cap this VLAN's WAN egress (upload), non-burstable.
# Requires the qos block to be enabled.
vlan "iot" {
  id = 20
  mdns_reflector = true  # IoT devices discoverable from trusted VLAN

  # bandwidth {
  #   upload_mbps   = 5
  #   download_mbps = 10
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
    ntp        = "10.99.2.2"

    # Give each Chromecast a static DHCP lease so allow_from rules can target it:
    # host {
    #   mac      = "aa:bb:cc:dd:ee:cc"
    #   ip       = "10.99.20.200"
    #   hostname = "chromecast-living-room"
    # }
  }

  # Cross-VLAN Chromecast: allow trusted VLAN to cast to Chromecasts on IoT.
  # Requires mdns_reflector=true on both VLANs (for device discovery).
  # TCP 8008/8009 = cast control, UDP 32768-61000 = media streaming.
  # allow_from "trusted" {
  #   tcp = ["10.99.20.200:8008", "10.99.20.200:8009"]
  #   udp = ["10.99.20.200:32768-61000"]
  # }
}

###
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
    ntp        = "10.99.2.2"
  }
}

###
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
    ntp        = "10.99.2.2"
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

# --- Managed switch (Sodola) ---
## nifty-filter has OPTIONAL support to manage your Sodola switch for
## you, uncomment the config and set the port assignments to the VLANs you desire.
#
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
    accept = "tagged-only"
    label  = "secondary switch"
    vlans {
      tagged = [20, 40]
    }
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
