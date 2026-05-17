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
    path = [ pkgs.iproute2 pkgs.systemd pkgs.procps ];
    script = ''
      if [ ! -f ${hclFile} ]; then
        echo "No HCL config found, skipping network config"
        exit 0
      fi

      # Read interface names and settings from HCL
      WAN_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} wan-name)
      TRUNK_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} trunk-name)
      MGMT_INTERFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} mgmt-name 2>/dev/null || true)
      ENABLE_IPV6=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} enable-ipv6)

      # Bring up interfaces
      [ -n "$WAN_INTERFACE" ] && ip link set "$WAN_INTERFACE" up
      [ -n "$TRUNK_INTERFACE" ] && ip link set "$TRUNK_INTERFACE" up
      [ -n "$MGMT_INTERFACE" ] && ip link set "$MGMT_INTERFACE" up

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
    '';
  };
}
