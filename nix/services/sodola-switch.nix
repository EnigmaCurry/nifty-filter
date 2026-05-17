# Sodola managed switch supervisor.
#
# Periodically reconciles the physical switch's VLAN/port config with
# the desired state declared in HCL. Communicates with the switch over
# HTTP (no local privileges needed for the main loop).
#
# Hardened: runs as dedicated 'sodola-switch' user with zero capabilities.
# The ExecStartPre runs as root (+ prefix) to insert an nft rule blocking
# LAN clients from reaching the switch management subnet directly.
#
# Optional — only started when packages.sodola-switch.enable is true.

{ lib, pkgs, cfg, nifty-filter, sodola-switch, hclFile, ... }:

let
  inherit (lib) mkIf;
in
{
  users.users.sodola-switch = {
    isSystemUser = true;
    group = "sodola-switch";
    description = "Sodola switch supervisor daemon";
  };
  users.groups.sodola-switch = {};

  systemd.services.nifty-sodola-switch = mkIf cfg.packages.sodola-switch.enable {
    description = "Supervise Sodola switch VLAN configuration";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "nifty-filter.service" ];

    path = [ pkgs.iproute2 pkgs.nftables ];
    serviceConfig = {
      Type = "simple";
      RuntimeDirectory = "nifty-filter";
      RuntimeDirectoryPreserve = "yes";
      ExecStartPre = [
        # Block forwarded traffic to the switch management subnet —
        # only the router itself (output chain) may reach the switch.
        "+${pkgs.bash}/bin/bash -c 'ROUTER_IP=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} switch-router-ip 2>/dev/null); if [ -n \"$ROUTER_IP\" ]; then NETWORK=$(echo $ROUTER_IP | ${pkgs.gnused}/bin/sed \"s|\\.[0-9]*/|.0/|\"); nft insert rule inet filter forward ip daddr $NETWORK drop comment \"block switch mgmt\" 2>/dev/null || true; fi'"
        # Assign the router IP to the management interface (runs as root)
        "+${pkgs.bash}/bin/bash -c 'IFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} switch-mgmt-iface 2>/dev/null); IP=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} switch-router-ip 2>/dev/null); if [ -n \"$IFACE\" ] && [ -n \"$IP\" ]; then ${pkgs.iproute2}/bin/ip addr add $IP dev $IFACE 2>/dev/null || true; fi'"
      ];
      ExecStart = "${sodola-switch}/bin/sodola-switch supervise --interval 60 --config ${hclFile} --state-file /run/nifty-filter/sodola-switch.json";
      Restart = "on-failure";
      RestartSec = "10s";

      # Privilege dropping
      User = "sodola-switch";
      Group = "sodola-switch";
      CapabilityBoundingSet = "";

      # Filesystem hardening
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      ReadOnlyPaths = [ hclFile ];

      # Kernel hardening
      NoNewPrivileges = true;
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      ProtectKernelLogs = true;
      ProtectControlGroups = true;

      # Misc hardening
      RestrictSUIDSGID = true;
      RestrictRealtime = true;
      RestrictNamespaces = true;
      LockPersonality = true;
    };
  };
}
