# Read-only NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Configuration: /var/nifty-filter/nifty-filter.env
# To reconfigure: run nifty-config
{ config, pkgs, lib, ... }:

let
  # Shell function to read a value from an env file, stripping surrounding quotes
  envget = ''
    envget() { grep -oP "^$1=\K.*" "$2" 2>/dev/null | sed "s/^\([\"']\)\(.*\)\1$/\2/"; }
  '';
in
{
  system.stateVersion = "25.05";
  networking.hostName = "nifty-filter";

  # Boot (filesystem mounts are in filesystem.nix, not here,
  # so the ISO can provide its own without conflicts)
  boot.loader.systemd-boot.enable = lib.mkDefault true;
  boot.loader.efi.canTouchEfiVariables = lib.mkDefault false;
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Serial console support (works alongside VGA)
  boot.kernelParams = [ "console=tty0" "console=ttyS0,115200n8" ];

  # Include common disk/filesystem drivers in initrd
  boot.initrd.availableKernelModules = [
    # Virtio (QEMU/KVM)
    "virtio_pci" "virtio_blk" "virtio_scsi" "virtio_net"
    # SATA/AHCI
    "ahci" "sd_mod"
    # NVMe
    "nvme"
    # USB storage
    "usb_storage" "uas" "xhci_pci" "ehci_pci"
    # SCSI
    "sr_mod"
  ];

  # Disable nix operations on the read-only system
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  nix.gc.automatic = false;

  # --- Nifty-filter firewall (reads /var/nifty-filter/nifty-filter.env at boot) ---
  services.nifty-filter.enable = true;
  services.nifty-filter.packages.sodola-switch.enable = true;
  services.nifty-filter.packages.nifty-dashboard.enable = true;
  services.nifty-filter.packages.iperf.enable = true;

  # Set hostname from /var/nifty-filter/nifty-filter.env at boot
  systemd.services.nifty-hostname = {
    description = "Set hostname from nifty-filter.env";
    wantedBy = [ "sysinit.target" ];
    before = [ "network-pre.target" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.hostname pkgs.gnugrep ];
    script = let d = "$"; in ''
      ${envget}
      ENV_FILE="/var/nifty-filter/nifty-filter.env"
      if [ -f "${d}ENV_FILE" ]; then
        NAME=${d}(envget HOSTNAME "${d}ENV_FILE")
        if [ -n "${d}NAME" ]; then
          hostname "${d}NAME"
        fi
      fi
    '';
  };

  # --- Networking ---
  # Interfaces are configured dynamically at boot from /var/nifty-filter/nifty-filter.env
  # No hardcoded interface names.
  # Interface rename rules (.link files) are in /var/nifty-filter/network/
  networking.useDHCP = false;

  # Reverse-path filtering (BCP 38 / RFC 3704)
  # Strict mode (1): drop packets whose source address would not be routed
  # back out the same interface they arrived on.
  boot.kernel.sysctl = {
    "net.ipv4.conf.default.rp_filter" = 1;
    "net.ipv4.conf.all.rp_filter" = 1;
  };

  # Remount root read-only after NixOS activation completes
  systemd.services.nifty-ro = {
    description = "Remount root filesystem read-only";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-tmpfiles-setup.service" ];
    unitConfig.ConditionKernelCommandLine = "!nifty.maintenance=1";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if ${pkgs.util-linux}/bin/findmnt -n -t ext4 / > /dev/null 2>&1; then
        ${pkgs.util-linux}/bin/mount -o remount,ro /
      fi
    '';
  };

  # In maintenance mode, remount /nix/store as read-write
  systemd.services.nifty-maintenance-rw = {
    description = "Remount nix store read-write in maintenance mode";
    wantedBy = [ "sysinit.target" ];
    before = [ "nix-daemon.service" ];
    unitConfig.DefaultDependencies = false;
    unitConfig.ConditionKernelCommandLine = "nifty.maintenance=1";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = [
        "${pkgs.util-linux}/bin/mount -o remount,rw /"
        "${pkgs.util-linux}/bin/mount -o remount,rw /nix/store"
      ];
    };
  };

  # Generate interface rename rules (.link files) from env at boot
  systemd.services.nifty-link = {
    description = "Generate interface rename rules from env";
    wantedBy = [ "sysinit.target" ];
    before = [ "systemd-udevd.service" "systemd-networkd.service" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.gnugrep pkgs.gnused ];
    script = let d = "$"; in ''
      ${envget}
      ENV_FILE="/var/nifty-filter/nifty-filter.env"

      if [ ! -f "${d}ENV_FILE" ]; then
        echo "No env file found, skipping link generation"
        exit 0
      fi

      ENABLED=${d}(envget ENABLED "${d}ENV_FILE")
      if [ "${d}ENABLED" != "true" ]; then
        echo "nifty-filter not enabled, skipping link generation"
        exit 0
      fi

      mkdir -p /run/systemd/network

      # Generate .link file helper — errors if MAC is missing
      make_link() {
        local mac="${d}1" name="${d}2"
        if [ -z "${d}mac" ]; then
          echo "ERROR: missing MAC address for interface '${d}name'" >&2
          exit 1
        fi
        cat > "/run/systemd/network/10-${d}name.link" <<EOF
      [Match]
      MACAddress=${d}mac

      [Link]
      Name=${d}name
      EOF
      }

      # Core interfaces
      WAN_MAC=${d}(envget WAN_MAC "${d}ENV_FILE")
      INTERFACE_WAN=${d}(envget INTERFACE_WAN "${d}ENV_FILE")
      make_link "${d}WAN_MAC" "${d}{INTERFACE_WAN:-wan}"

      TRUNK_MAC=${d}(envget TRUNK_MAC "${d}ENV_FILE")
      INTERFACE_TRUNK=${d}(envget INTERFACE_TRUNK "${d}ENV_FILE")
      make_link "${d}TRUNK_MAC" "${d}{INTERFACE_TRUNK:-trunk}"

      MGMT_MAC=${d}(envget MGMT_MAC "${d}ENV_FILE")
      INTERFACE_MGMT=${d}(envget INTERFACE_MGMT "${d}ENV_FILE")
      if [ -n "${d}INTERFACE_MGMT" ]; then
        make_link "${d}MGMT_MAC" "${d}INTERFACE_MGMT"
      fi

      # Extra interfaces (comma-separated MAC=name pairs)
      EXTRA_LINKS=${d}(envget EXTRA_LINKS "${d}ENV_FILE")
      if [ -n "${d}EXTRA_LINKS" ]; then
        IFS=',' read -ra PAIRS <<< "${d}EXTRA_LINKS"
        for pair in "${d}{PAIRS[@]}"; do
          pair=${d}(echo "${d}pair" | sed 's/^ *//;s/ *$//')
          mac="${d}{pair%%=*}"
          name="${d}{pair##*=}"
          make_link "${d}mac" "${d}name"
        done
      fi
    '';
  };

  # Configure WAN, trunk/VLANs, and optional mgmt from env files at boot
  # nifty-network is the sole owner of runtime .network and .netdev files.
  systemd.services.nifty-network = {
    description = "Configure network interfaces from env files";
    wantedBy = [ "multi-user.target" ];
    before = [ "network.target" "nifty-filter.service" ];
    after = [ "network-pre.target" "nifty-filter-init.service" ];
    wants = [ "network-pre.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.systemd pkgs.procps pkgs.gnugrep pkgs.gnused pkgs.gawk ];
    script = let d = "$"; in ''
      ${envget}
      ENV_FILE="/var/nifty-filter/nifty-filter.env"
      if [ ! -f "${d}ENV_FILE" ]; then
        echo "No nifty-filter.env found, skipping network config"
        exit 0
      fi

      ENABLED=${d}(envget ENABLED "${d}ENV_FILE")
      ENABLED=${d}{ENABLED:-false}
      if [ "${d}ENABLED" != "true" ]; then
        echo "nifty-filter not enabled, skipping network config"
        exit 0
      fi

      INTERFACE_WAN=${d}(envget INTERFACE_WAN "${d}ENV_FILE")
      # Trunk: prefer INTERFACE_TRUNK, fall back to INTERFACE_LAN
      INTERFACE_TRUNK=${d}(envget INTERFACE_TRUNK "${d}ENV_FILE")
      [ -z "${d}INTERFACE_TRUNK" ] && INTERFACE_TRUNK=${d}(envget INTERFACE_LAN "${d}ENV_FILE")
      # WAN protocol enablement (prefer WAN_ENABLE_*, fall back to legacy ENABLE_*)
      ENABLE_IPV4=${d}(envget WAN_ENABLE_IPV4 "${d}ENV_FILE")
      [ -z "${d}ENABLE_IPV4" ] && ENABLE_IPV4=${d}(envget ENABLE_IPV4 "${d}ENV_FILE")
      ENABLE_IPV4=${d}{ENABLE_IPV4:-true}
      ENABLE_IPV6=${d}(envget WAN_ENABLE_IPV6 "${d}ENV_FILE")
      [ -z "${d}ENABLE_IPV6" ] && ENABLE_IPV6=${d}(envget ENABLE_IPV6 "${d}ENV_FILE")
      ENABLE_IPV6=${d}{ENABLE_IPV6:-false}

      VLAN_AWARE_SWITCH=${d}(envget VLAN_AWARE_SWITCH "${d}ENV_FILE")
      VLAN_AWARE_SWITCH=${d}{VLAN_AWARE_SWITCH:-false}
      VLANS=${d}(envget VLANS "${d}ENV_FILE")

      # Determine VLAN IDs: explicit VLANS=, then auto-detect from VLAN_N_*, then VLAN 1
      if [ -n "${d}VLANS" ]; then
        VLAN_IDS="${d}VLANS"
      else
        VLAN_IDS=${d}(grep -oP '^VLAN_\K[0-9]+(?=_)' "${d}ENV_FILE" | sort -un | paste -sd,)
        [ -z "${d}VLAN_IDS" ] && VLAN_IDS="1"
      fi

      mkdir -p /run/systemd/network

      # --- WAN (DHCP client) ---
      ip link set "${d}INTERFACE_WAN" up

      WAN_NETWORK="[Network]"
      if [ "${d}ENABLE_IPV4" = "true" ]; then
        WAN_NETWORK="${d}WAN_NETWORK
      DHCP=ipv4"
      fi
      if [ "${d}ENABLE_IPV6" = "true" ]; then
        WAN_NETWORK="${d}WAN_NETWORK
      IPv6AcceptRA=yes
      IPv6Forwarding=no"
      fi

      cat > /run/systemd/network/10-wan.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_WAN

      ${d}WAN_NETWORK

      [DHCPv4]
      UseDNS=yes

      [IPv6AcceptRA]
      UseDNS=yes
      DHCPv6Client=always

      [DHCPv6]
      UseDNS=no
      PrefixDelegationHint=::/60
      NETEOF

      # Ensure WAN accepts RAs despite forwarding (must override after networkd)
      if [ "${d}ENABLE_IPV6" = "true" ]; then
        mkdir -p /etc/systemd/system/systemd-networkd.service.d
        cat > /etc/systemd/system/systemd-networkd.service.d/accept-ra.conf <<RAEOF
      [Service]
      ExecStartPost=/bin/sh -c 'sleep 1 && /run/current-system/sw/bin/sysctl -w net.ipv6.conf.${d}INTERFACE_WAN.accept_ra=2 net.ipv6.conf.${d}INTERFACE_WAN.forwarding=0'
      RAEOF
        systemctl daemon-reload
      fi

      # --- Trunk + VLANs ---
      ip link set "${d}INTERFACE_TRUNK" up

      if [ "${d}VLAN_AWARE_SWITCH" = "true" ]; then
        # VLAN-aware mode: trunk carries no IP, VLAN subinterfaces get addresses
        TRUNK_VLAN_LINES=""
        IFS=',' read -ra VID_ARRAY <<< "${d}VLAN_IDS"
        for VID in "${d}{VID_ARRAY[@]}"; do
          VID=${d}(echo "${d}VID" | tr -d ' ')
          VLAN_NAME=${d}(envget VLAN_${d}{VID}_NAME "${d}ENV_FILE")
          if [ -n "${d}VLAN_NAME" ]; then
            VLAN_IFACE="${d}VLAN_NAME"
          else
            VLAN_IFACE="${d}INTERFACE_TRUNK.${d}VID"
          fi
          TRUNK_VLAN_LINES="${d}{TRUNK_VLAN_LINES}
      VLAN=${d}VLAN_IFACE"

          # Create .netdev for this VLAN
          cat > /run/systemd/network/20-${d}VLAN_IFACE.netdev <<NETEOF
      [NetDev]
      Name=${d}VLAN_IFACE
      Kind=vlan

      [VLAN]
      Id=${d}VID
      NETEOF

          # Create .network for this VLAN subinterface
          VLAN_NETWORK="[Network]"
          SUBNET_V4=${d}(envget VLAN_${d}{VID}_SUBNET_IPV4 "${d}ENV_FILE")
          SUBNET_V6=${d}(envget VLAN_${d}{VID}_SUBNET_IPV6 "${d}ENV_FILE")

          if [ "${d}ENABLE_IPV4" = "true" ] && [ -n "${d}SUBNET_V4" ]; then
            VLAN_NETWORK="${d}VLAN_NETWORK
      Address=${d}SUBNET_V4"
          fi
          if [ -n "${d}SUBNET_V6" ]; then
            VLAN_NETWORK="${d}VLAN_NETWORK
      Address=${d}SUBNET_V6
      IPv6SendRA=yes"
          fi

          cat > /run/systemd/network/20-${d}VLAN_IFACE.network <<NETEOF
      [Match]
      Name=${d}VLAN_IFACE

      ${d}VLAN_NETWORK
      NETEOF

          # IPv6 RA config for this VLAN
          if [ -n "${d}SUBNET_V6" ]; then
            DHCPV6_EN=${d}(envget VLAN_${d}{VID}_DHCPV6_ENABLED "${d}ENV_FILE")
            DHCPV6_EN=${d}{DHCPV6_EN:-false}
            RA_MANAGED="no"
            RA_OTHER="no"
            RA_AUTONOMOUS="yes"
            if [ "${d}DHCPV6_EN" = "true" ]; then
              RA_MANAGED="yes"
              RA_OTHER="yes"
              RA_AUTONOMOUS="no"
            fi
            cat >> /run/systemd/network/20-${d}VLAN_IFACE.network <<NETEOF

      [IPv6SendRA]
      Managed=${d}RA_MANAGED
      OtherInformation=${d}RA_OTHER

      [IPv6Prefix]
      Prefix=${d}SUBNET_V6
      Autonomous=${d}RA_AUTONOMOUS
      NETEOF
          fi
        done

        # Trunk .network: no address, just VLAN membership
        cat > /run/systemd/network/10-trunk.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_TRUNK

      [Link]
      RequiredForOnline=no

      [Network]
      ${d}TRUNK_VLAN_LINES
      NETEOF

      else
        # Simple mode (VLAN 1): trunk gets the LAN IP directly
        # Support both new VLAN_1_SUBNET_* and legacy SUBNET_LAN* vars
        SUBNET_V4=${d}(envget VLAN_1_SUBNET_IPV4 "${d}ENV_FILE")
        [ -z "${d}SUBNET_V4" ] && SUBNET_V4=${d}(envget SUBNET_LAN_IPV4 "${d}ENV_FILE")
        [ -z "${d}SUBNET_V4" ] && SUBNET_V4=${d}(envget SUBNET_LAN "${d}ENV_FILE")
        SUBNET_V6=${d}(envget VLAN_1_SUBNET_IPV6 "${d}ENV_FILE")
        [ -z "${d}SUBNET_V6" ] && SUBNET_V6=${d}(envget SUBNET_LAN_IPV6 "${d}ENV_FILE")

        TRUNK_NETWORK="[Network]"
        if [ "${d}ENABLE_IPV4" = "true" ] && [ -n "${d}SUBNET_V4" ]; then
          TRUNK_NETWORK="${d}TRUNK_NETWORK
      Address=${d}SUBNET_V4"
        fi
        if [ -n "${d}SUBNET_V6" ]; then
          TRUNK_NETWORK="${d}TRUNK_NETWORK
      Address=${d}SUBNET_V6
      IPv6SendRA=yes"
        fi

        cat > /run/systemd/network/10-trunk.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_TRUNK

      ${d}TRUNK_NETWORK
      NETEOF

        # IPv6 RA config for simple mode
        if [ -n "${d}SUBNET_V6" ]; then
          DHCPV6_EN=${d}(envget VLAN_1_DHCPV6_ENABLED "${d}ENV_FILE")
          [ -z "${d}DHCPV6_EN" ] && DHCPV6_EN=${d}(envget DHCPV6_ENABLED "${d}ENV_FILE")
          DHCPV6_EN=${d}{DHCPV6_EN:-false}
          RA_MANAGED="no"
          RA_OTHER="no"
          RA_AUTONOMOUS="yes"
          if [ "${d}DHCPV6_EN" = "true" ]; then
            RA_MANAGED="yes"
            RA_OTHER="yes"
            RA_AUTONOMOUS="no"
          fi
          cat >> /run/systemd/network/10-trunk.network <<NETEOF

      [IPv6SendRA]
      Managed=${d}RA_MANAGED
      OtherInformation=${d}RA_OTHER

      [IPv6Prefix]
      Prefix=${d}SUBNET_V6
      Autonomous=${d}RA_AUTONOMOUS
      NETEOF
        fi
      fi

      # Optional management interface (static IP, no DHCP server)
      INTERFACE_MGMT=${d}(envget INTERFACE_MGMT "${d}ENV_FILE")
      SUBNET_MGMT=${d}(envget SUBNET_MGMT "${d}ENV_FILE")
      if [ -n "${d}INTERFACE_MGMT" ] && [ -n "${d}SUBNET_MGMT" ]; then
        ip link set "${d}INTERFACE_MGMT" up
        cat > /run/systemd/network/10-mgmt.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_MGMT

      [Network]
      Address=${d}SUBNET_MGMT
      LinkLocalAddressing=no
      IPv6AcceptRA=no
      NETEOF
      sysctl -w net.ipv6.conf.${d}INTERFACE_MGMT.disable_ipv6=1
      fi

      # Restart networkd to pick up the new configs
      networkctl reload || systemctl restart systemd-networkd
    '';
  };

  # Use systemd-networkd for runtime network config
  networking.useNetworkd = true;

  # Static resolv.conf pointing to local dnsmasq (no resolvconf needed)
  networking.resolvconf.enable = false;
  environment.etc."resolv.conf".text = ''
    nameserver 127.0.0.1
  '';

  # --- dnsmasq: DHCP + DNS for LAN ---
  # Config is generated at boot from /var/nifty-filter/nifty-filter.env
  # Forwards DNS to upstream (Cloudflare by default).
  services.resolved.enable = false;

  systemd.services.nifty-dnsmasq = {
    description = "dnsmasq DHCP and DNS server from env file";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" ];
    serviceConfig = {
      Type = "forking";
      PIDFile = "/run/dnsmasq.pid";
      ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq -C /run/dnsmasq.conf";
      ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
      Restart = "on-failure";
    };
    path = [ pkgs.gnugrep pkgs.gnused pkgs.coreutils pkgs.gawk ];
    preStart = let d = "$"; in ''
      ${envget}
      ENV_FILE="/var/nifty-filter/nifty-filter.env"
      if [ ! -f "${d}ENV_FILE" ]; then
        echo "No nifty-filter.env found, writing minimal DNS-only config"
        cat > /run/dnsmasq.conf <<DNSEOF
      pid-file=/run/dnsmasq.pid
      listen-address=127.0.0.1
      bind-interfaces
      no-resolv
      server=1.1.1.1
      server=1.0.0.1
      DNSEOF
        exit 0
      fi

      # Trunk: prefer INTERFACE_TRUNK, fall back to INTERFACE_LAN
      INTERFACE_TRUNK=${d}(envget INTERFACE_TRUNK "${d}ENV_FILE")
      [ -z "${d}INTERFACE_TRUNK" ] && INTERFACE_TRUNK=${d}(envget INTERFACE_LAN "${d}ENV_FILE")

      VLAN_AWARE_SWITCH=${d}(envget VLAN_AWARE_SWITCH "${d}ENV_FILE")
      VLAN_AWARE_SWITCH=${d}{VLAN_AWARE_SWITCH:-false}
      VLANS=${d}(envget VLANS "${d}ENV_FILE")

      # Determine VLAN IDs: explicit VLANS=, then auto-detect from VLAN_N_*, then VLAN 1
      if [ -n "${d}VLANS" ]; then
        VLAN_IDS="${d}VLANS"
      else
        VLAN_IDS=${d}(grep -oP '^VLAN_\K[0-9]+(?=_)' "${d}ENV_FILE" | sort -un | paste -sd,)
        [ -z "${d}VLAN_IDS" ] && VLAN_IDS="1"
      fi

      # Build upstream DNS server lines
      UPSTREAM_DNS=${d}(envget DHCP_UPSTREAM_DNS "${d}ENV_FILE")
      [ -z "${d}UPSTREAM_DNS" ] && UPSTREAM_DNS=${d}(envget DHCP_DNS "${d}ENV_FILE")
      UPSTREAM_DNS=${d}{UPSTREAM_DNS:-1.1.1.1, 1.0.0.1}
      DNS_SERVERS=""
      IFS=',' read -ra DNS_ARRAY <<< "${d}UPSTREAM_DNS"
      for dns in "${d}{DNS_ARRAY[@]}"; do
        dns=${d}(echo "${d}dns" | tr -d ' ')
        DNS_SERVERS="${d}{DNS_SERVERS}
      server=${d}dns"
      done

      mkdir -p /var/lib/dnsmasq
      cat > /run/dnsmasq.conf <<DNSEOF
      # Generated from /var/nifty-filter/nifty-filter.env
      pid-file=/run/dnsmasq.pid

      # DNS
      no-resolv
      ${d}DNS_SERVERS
      domain-needed
      bogus-priv
      cache-size=1000

      # Localhost listeners
      listen-address=::1
      listen-address=127.0.0.1
      bind-dynamic

      dhcp-leasefile=/var/lib/dnsmasq/dnsmasq.leases
      log-dhcp
      DNSEOF

      # Generate per-VLAN DHCP/DNS config
      IFS=',' read -ra VID_ARRAY <<< "${d}VLAN_IDS"
      for VID in "${d}{VID_ARRAY[@]}"; do
        VID=${d}(echo "${d}VID" | tr -d ' ')

        # Determine interface name for this VLAN
        VLAN_NAME=${d}(envget VLAN_${d}{VID}_NAME "${d}ENV_FILE")
        if [ -n "${d}VLAN_NAME" ]; then
          VLAN_IFACE="${d}VLAN_NAME"
        elif [ "${d}VID" = "1" ]; then
          VLAN_IFACE="${d}INTERFACE_TRUNK"
        else
          VLAN_IFACE="${d}INTERFACE_TRUNK.${d}VID"
        fi

        # Read per-VLAN config (with legacy fallback for VLAN 1)
        DHCP_EN=${d}(envget VLAN_${d}{VID}_DHCP_ENABLED "${d}ENV_FILE")
        if [ -z "${d}DHCP_EN" ] && [ "${d}VID" = "1" ]; then
          DHCP_EN=${d}(envget DHCP4_ENABLED "${d}ENV_FILE")
        fi
        DHCP_EN=${d}{DHCP_EN:-true}

        POOL_START=${d}(envget VLAN_${d}{VID}_DHCP_POOL_START "${d}ENV_FILE")
        [ -z "${d}POOL_START" ] && [ "${d}VID" = "1" ] && POOL_START=${d}(envget DHCP_POOL_START "${d}ENV_FILE")
        POOL_END=${d}(envget VLAN_${d}{VID}_DHCP_POOL_END "${d}ENV_FILE")
        [ -z "${d}POOL_END" ] && [ "${d}VID" = "1" ] && POOL_END=${d}(envget DHCP_POOL_END "${d}ENV_FILE")
        DHCP_RTR=${d}(envget VLAN_${d}{VID}_DHCP_ROUTER "${d}ENV_FILE")
        [ -z "${d}DHCP_RTR" ] && [ "${d}VID" = "1" ] && DHCP_RTR=${d}(envget DHCP_ROUTER "${d}ENV_FILE")
        VLAN_DNS=${d}(envget VLAN_${d}{VID}_DHCP_DNS "${d}ENV_FILE")
        [ -z "${d}VLAN_DNS" ] && VLAN_DNS="${d}DHCP_RTR"

        # Add interface + listen-address
        cat >> /run/dnsmasq.conf <<DNSEOF

      # VLAN ${d}VID (${d}VLAN_IFACE)
      interface=${d}VLAN_IFACE
      DNSEOF
        if [ -n "${d}DHCP_RTR" ]; then
          echo "listen-address=${d}DHCP_RTR" >> /run/dnsmasq.conf
        fi

        # DHCPv4
        if [ "${d}DHCP_EN" = "true" ] && [ -n "${d}POOL_START" ] && [ -n "${d}POOL_END" ]; then
          cat >> /run/dnsmasq.conf <<DNSEOF
      dhcp-range=interface:${d}VLAN_IFACE,${d}POOL_START,${d}POOL_END,24h
      dhcp-option=interface:${d}VLAN_IFACE,option:router,${d}DHCP_RTR
      dhcp-option=interface:${d}VLAN_IFACE,option:dns-server,${d}VLAN_DNS
      DNSEOF
        fi

        # DHCPv6
        DHCPV6_EN=${d}(envget VLAN_${d}{VID}_DHCPV6_ENABLED "${d}ENV_FILE")
        if [ -z "${d}DHCPV6_EN" ] && [ "${d}VID" = "1" ]; then
          DHCPV6_EN=${d}(envget DHCPV6_ENABLED "${d}ENV_FILE")
        fi
        DHCPV6_EN=${d}{DHCPV6_EN:-false}
        if [ "${d}DHCPV6_EN" = "true" ]; then
          V6_START=${d}(envget VLAN_${d}{VID}_DHCPV6_POOL_START "${d}ENV_FILE")
          [ -z "${d}V6_START" ] && [ "${d}VID" = "1" ] && V6_START=${d}(envget DHCPV6_POOL_START "${d}ENV_FILE")
          V6_END=${d}(envget VLAN_${d}{VID}_DHCPV6_POOL_END "${d}ENV_FILE")
          [ -z "${d}V6_END" ] && [ "${d}VID" = "1" ] && V6_END=${d}(envget DHCPV6_POOL_END "${d}ENV_FILE")
          SUBNET_V6=${d}(envget VLAN_${d}{VID}_SUBNET_IPV6 "${d}ENV_FILE")
          [ -z "${d}SUBNET_V6" ] && [ "${d}VID" = "1" ] && SUBNET_V6=${d}(envget SUBNET_LAN_IPV6 "${d}ENV_FILE")
          ROUTER_V6=${d}(echo "${d}SUBNET_V6" | cut -d/ -f1)
          # Use per-VLAN DNS for DHCPv6 (fall back to router's IPv6 address)
          VLAN_DNS_V6=${d}(envget VLAN_${d}{VID}_DHCP_DNS "${d}ENV_FILE")
          [ -z "${d}VLAN_DNS_V6" ] && VLAN_DNS_V6="${d}ROUTER_V6"
          if [ -n "${d}V6_START" ] && [ -n "${d}V6_END" ]; then
            cat >> /run/dnsmasq.conf <<DNSEOF
      dhcp-range=interface:${d}VLAN_IFACE,${d}V6_START,${d}V6_END,64,24h
      dhcp-option=interface:${d}VLAN_IFACE,option6:dns-server,[${d}VLAN_DNS_V6]
      enable-ra
      ra-param=${d}VLAN_IFACE,60,600
      DNSEOF
          fi
        fi
      done
    '';
  };

  # --- SSH ---
  services.openssh = {
    enable = true;
    # Persist host keys on /var so they survive image upgrades
    hostKeys = [
      { path = "/var/nifty-filter/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
      { path = "/var/nifty-filter/ssh/ssh_host_rsa_key"; type = "rsa"; bits = 4096; }
    ];
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      X11Forwarding = false;
      MaxAuthTries = 3;
      ClientAliveInterval = 300;
      ClientAliveCountMax = 2;
    };
  };

  # --- User account ---
  # SSH keys are provisioned at runtime from /var, not at build time.
  # The installer ensures keys exist before writing to disk.
  users.allowNoPasswordLogin = true;
  users.mutableUsers = false;
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    # SSH authorized keys live in ~/.ssh/authorized_keys (standard path).
    # Since /home is bind-mounted from /var/home, this persists across reboots.
    # Use ssh-copy-id to add keys.
  };
  security.sudo.wheelNeedsPassword = false;

  # Allow wheel users to reboot/poweroff without sudo
  security.polkit.extraConfig = ''
    polkit.addRule(function(action, subject) {
      if ((action.id == "org.freedesktop.login1.reboot" ||
           action.id == "org.freedesktop.login1.reboot-multiple-sessions" ||
           action.id == "org.freedesktop.login1.power-off" ||
           action.id == "org.freedesktop.login1.power-off-multiple-sessions") &&
          subject.isInGroup("wheel")) {
        return polkit.Result.YES;
      }
    });
  '';

  # --- Minimal packages ---
  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "nifty-config" ''exec nifty-filter config "$@"'')
    (writeShellScriptBin "nifty-maintenance" ''exec nifty-filter maintenance "$@"'')
    (writeShellScriptBin "nifty-upgrade" ''exec nifty-filter upgrade "$@"'')
    git
    vim
    htop
    ethtool
    tcpdump
    iproute2
    dig
  ];

  # Maintenance mode indicator — modify PS1 and show warning
  environment.interactiveShellInit = ''
    if grep -q 'nifty.maintenance=1' /proc/cmdline 2>/dev/null; then
      export PS1='\[\e[1;31m\][MAINTENANCE]\[\e[0m\] \u@\h:\w\$ '
      echo ""
      echo -e "\e[1;31m  *** MAINTENANCE MODE — root filesystem is READ-WRITE ***\e[0m"
      echo ""
      echo "  Upgrade system:  nifty-upgrade"
      echo "  Return to normal: systemctl reboot"
      echo ""
    else
      echo ""
      echo "  Configure:  nifty-config"
      echo "  Upgrade:    nifty-upgrade"
      echo ""
    fi
  '';

  # Auto-login on console (SSH is primary access; console is for emergencies)
  systemd.services."getty@tty1" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.util-linux}/bin/agetty --autologin admin --noclear --keep-baud %I 115200,38400,9600 $TERM"
    ];
  };
  # Serial console auto-login
  systemd.services."serial-getty@ttyS0" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.util-linux}/bin/agetty --autologin admin --keep-baud ttyS0 115200,38400,9600 vt100"
    ];
  };
  # Pre-login banner using agetty built-in escapes (works on read-only root)
  environment.etc."issue".text = lib.mkDefault ''

    \e[1mnifty-filter\e[0m (\n) \l
    wan: \4{wan}
    trunk: \4{trunk}

  '';

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
}
