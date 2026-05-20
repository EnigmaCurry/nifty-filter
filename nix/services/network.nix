# Network interface configuration service.
#
# Reads interface definitions from HCL config, brings up WAN/trunk/mgmt
# interfaces, generates systemd-networkd .netdev and .network files for
# VLANs, and restarts networkd to apply. Also configures IPv6 RA acceptance
# on the WAN interface when IPv6 is enabled.
#
# Runs as root (requires interface manipulation).

{ pkgs, nifty-filter, hclFile, ... }:

{
  # Configure WAN, trunk/VLANs, and optional mgmt from HCL config at boot
  systemd.services.nifty-network = {
    description = "Configure network interfaces from HCL config";
    wantedBy = [ "multi-user.target" ];
    before = [ "network.target" "nifty-filter.service" ];
    after = [ "network-pre.target" "nifty-filter-init.service" ];
    wants = [ "network-pre.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.systemd pkgs.procps pkgs.gnugrep ];
    script = ''
      if [ ! -f ${hclFile} ]; then
        echo "No HCL config found, skipping network config"
        exit 0
      fi

      # Rename an interface by MAC if needed: ensure_name <desired-name> <mac>
      ensure_name() {
        local WANT="$1" MAC="$2"
        [ -z "$MAC" ] && return 0
        MAC=$(echo "$MAC" | tr '[:upper:]' '[:lower:]')
        # Already exists with correct name?
        if ip link show "$WANT" &>/dev/null; then
          return 0
        fi
        # Find interface with this MAC
        local CURRENT
        CURRENT=$(ip -o link show | grep -i "link/ether $MAC " | grep -oP '^\d+: \K[^:@]+' | head -1)
        if [ -z "$CURRENT" ]; then
          echo "WARNING: No interface found with MAC $MAC for $WANT"
          return 1
        fi
        echo "Renaming $CURRENT -> $WANT (MAC: $MAC)"
        ip link set "$CURRENT" down
        ip link set "$CURRENT" name "$WANT"
        return 0
      }

      # Read interface names and settings from HCL
      WAN_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} wan-name)
      TRUNK_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} trunk-name)
      MGMT_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} mgmt-name 2>/dev/null || true)
      ENABLE_IPV6=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} enable-ipv6)

      # Rename interfaces by MAC if .link files haven't taken effect yet
      WAN_MAC=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} wan-mac 2>/dev/null || true)
      TRUNK_MAC=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} trunk-mac 2>/dev/null || true)
      MGMT_MAC=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} mgmt-mac 2>/dev/null || true)
      [ -n "$WAN_MAC" ] && ensure_name "$WAN_INTERFACE" "$WAN_MAC"
      [ -n "$TRUNK_MAC" ] && ensure_name "$TRUNK_INTERFACE" "$TRUNK_MAC"
      [ -n "$MGMT_MAC" ] && [ -n "$MGMT_INTERFACE" ] && ensure_name "$MGMT_INTERFACE" "$MGMT_MAC"

      # Rename and bring up dedicated VLAN interfaces
      VLAN_IFACE_MACS=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} vlan-interface-macs 2>/dev/null || true)
      for pair in $VLAN_IFACE_MACS; do
        VNAME="''${pair%%=*}"
        VMAC="''${pair#*=}"
        ensure_name "$VNAME" "$VMAC" || true
      done

      # Dedicated VLAN interfaces (not on trunk)
      VLAN_INTERFACES=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} vlan-interfaces 2>/dev/null || true)

      # Bring up interfaces
      [ -n "$WAN_INTERFACE" ] && ip link set "$WAN_INTERFACE" up
      [ -n "$TRUNK_INTERFACE" ] && ip link set "$TRUNK_INTERFACE" up
      [ -n "$MGMT_INTERFACE" ] && ip link set "$MGMT_INTERFACE" up 2>/dev/null || true
      for iface in $VLAN_INTERFACES; do
        ip link set "$iface" up 2>/dev/null || echo "WARNING: Interface $iface not found, skipping"
      done

      # Generate networkd config files
      mkdir -p /run/systemd/network
      ${nifty-filter}/bin/nifty-filter generate networkd --config ${hclFile} --output-dir /run/systemd/network

      # Ensure WAN accepts RAs despite forwarding (must override after networkd)
      if [ "$ENABLE_IPV6" = "true" ] && [ -n "$WAN_INTERFACE" ]; then
        mkdir -p /run/systemd/system/systemd-networkd.service.d
        cat > /run/systemd/system/systemd-networkd.service.d/accept-ra.conf <<RAEOF
      [Service]
      ExecStartPost=/bin/sh -c 'sleep 1 && /run/current-system/sw/bin/sysctl -w net.ipv6.conf.$WAN_INTERFACE.accept_ra=2 net.ipv6.conf.$WAN_INTERFACE.forwarding=0'
      RAEOF
        systemctl daemon-reload
      fi

      # Disable IPv6 on management interface
      if [ -n "$MGMT_INTERFACE" ]; then
        sysctl -w net.ipv6.conf.$MGMT_INTERFACE.disable_ipv6=1
      fi

      # Restart networkd to pick up new .netdev and .network configs
      # A full restart is required for new .netdev files (VLAN interfaces);
      # networkctl reload only re-applies .network files.
      systemctl restart systemd-networkd

      # Wait for VLAN subinterfaces to be created by networkd
      # (nifty-filter needs them to exist for nftables rules)
      TRUNK_VLANS=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} trunk-vlan-names 2>/dev/null || true)
      if [ -n "$TRUNK_VLANS" ]; then
        for attempt in $(seq 1 20); do
          ALL_EXIST=1
          for vlan_iface in $TRUNK_VLANS; do
            if ! ip link show "$vlan_iface" &>/dev/null; then
              ALL_EXIST=0
              break
            fi
          done
          [ "$ALL_EXIST" = "1" ] && break
          sleep 0.25
        done
        if [ "$ALL_EXIST" != "1" ]; then
          echo "WARNING: Not all VLAN interfaces appeared after 5s"
          ip link show | grep -E '^[0-9]+:' || true
        fi
      fi
    '';
  };
}
