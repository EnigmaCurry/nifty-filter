# avahi-daemon mDNS reflector.
#
# Reflects mDNS (.local) queries between VLANs that have mdns_reflector=true
# in the HCL config. Does not publish any services — only reflects.
#
# If no VLANs have mdns_reflector enabled, the service skips startup gracefully.

{ pkgs, nifty-filter, hclFile, ... }:

{
  users.groups.avahi = {};
  users.users.avahi = {
    isSystemUser = true;
    group = "avahi";
    description = "avahi-daemon privilege separation user";
  };

  systemd.services.nifty-avahi = {
    description = "avahi-daemon mDNS reflector";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" "nifty-filter.service" ];

    serviceConfig = {
      Type = "simple";
      ExecStart = "${pkgs.avahi}/bin/avahi-daemon -f /run/avahi-daemon/avahi-daemon.conf --no-rlimits";
      Restart = "on-failure";
      RestartSec = "5s";

      RuntimeDirectory = "avahi-daemon";
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      NoNewPrivileges = true;
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      ProtectKernelLogs = true;
      ProtectControlGroups = true;
      RestrictSUIDSGID = true;
      RestrictRealtime = true;
      LockPersonality = true;

      ExecStartPre = let
        preStartScript = pkgs.writeShellScript "nifty-avahi-pre" ''
          if [ ! -f ${hclFile} ]; then
            echo "No HCL config found, skipping avahi."
            exit 1
          fi
          IFACES=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} mdns-interfaces 2>/dev/null || true)
          if [ -z "$IFACES" ]; then
            echo "No mDNS reflector interfaces configured, skipping avahi."
            exit 1
          fi
          ${nifty-filter}/bin/nifty-filter generate avahi --config ${hclFile} --output /run/avahi-daemon/avahi-daemon.conf
        '';
      in "+${preStartScript}";
    };
  };
}
