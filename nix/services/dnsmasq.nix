# dnsmasq DHCP and DNS server.
#
# Hardened: runs as dedicated 'dnsmasq' user with only the capabilities
# needed to bind privileged ports and send raw DHCP packets:
#   CAP_NET_BIND_SERVICE, CAP_NET_RAW, CAP_NET_ADMIN
#
# Filesystem is read-only except /var/lib/dnsmasq (lease file).
# Config is generated from HCL by a root ExecStartPre and written
# to /run/dnsmasq/dnsmasq.conf.

{ pkgs, nifty-filter, hclFile, ... }:

{
  users.users.dnsmasq = {
    isSystemUser = true;
    group = "dnsmasq";
    description = "dnsmasq DHCP/DNS daemon";
  };
  users.groups.dnsmasq = {};

  systemd.services.nifty-dnsmasq = {
    description = "dnsmasq DHCP and DNS server";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" ];
    serviceConfig = {
      Type = "forking";
      PIDFile = "/run/dnsmasq/dnsmasq.pid";
      ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq -C /run/dnsmasq/dnsmasq.conf";
      ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
      Restart = "on-failure";

      # Privilege dropping
      User = "dnsmasq";
      Group = "dnsmasq";
      AmbientCapabilities = "CAP_NET_BIND_SERVICE CAP_NET_RAW CAP_NET_ADMIN";
      CapabilityBoundingSet = "CAP_NET_BIND_SERVICE CAP_NET_RAW CAP_NET_ADMIN";

      # Filesystem hardening
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      StateDirectory = "dnsmasq";
      RuntimeDirectory = "dnsmasq";

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
      MemoryDenyWriteExecute = true;
      LockPersonality = true;

      ExecStartPre = let
        preStartScript = pkgs.writeShellScript "nifty-dnsmasq-pre" ''
          mkdir -p /var/lib/dnsmasq
          chown -R dnsmasq:dnsmasq /var/lib/dnsmasq
          if [ -f ${hclFile} ]; then
            ${nifty-filter}/bin/nifty-filter generate dnsmasq --config ${hclFile} --output /run/dnsmasq/dnsmasq.conf
          else
            ${nifty-filter}/bin/nifty-filter generate dnsmasq-minimal --output /run/dnsmasq/dnsmasq.conf
          fi
        '';
      in "+${preStartScript}";
    };
  };
}
