# Dashboard web UI and its state dump dependency.
#
# Architecture:
#   nifty-state-dump (long-running, every 3s)
#     - Dedicated system user (nifty-state) with CAP_NET_ADMIN
#     - Writes JSON and text snapshots to /run/nifty-state/
#     - Files are world-readable (644), directory is 755
#
#   nifty-dashboard (long-running web server)
#     - Zero capabilities (CapabilityBoundingSet="")
#     - Reads state exclusively from dump files in /run/nifty-state/
#     - Rejects data older than 15 seconds as stale
#     - DynamicUser with strict filesystem sandboxing
#
# This separation ensures the dashboard cannot modify firewall rules,
# interfaces, or traffic shaping even if fully compromised.

{ lib, pkgs, cfg, nifty-filter, nifty-dashboard, hclFile, ... }:

let
  inherit (lib) mkIf;
in
{
  users.users.nifty-state = {
    isSystemUser = true;
    group = "nifty-state";
  };
  users.groups.nifty-state = {};

  # Periodic state dump for dashboard (read-only snapshot of nft/tc/ip state)
  # Runs with CAP_NET_ADMIN so the dashboard itself needs zero capabilities.
  # Long-running service with built-in sleep loop (avoids timer log spam).
  systemd.services.nifty-state-dump = mkIf cfg.packages.nifty-dashboard.enable {
    description = "Dump network state for dashboard";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-config-sha.service" ];

    path = [ pkgs.iproute2 pkgs.nftables pkgs.coreutils pkgs.bash ];
    serviceConfig = {
      Type = "simple";
      Restart = "on-failure";
      RestartSec = "3s";

      # Minimal privileges: only CAP_NET_ADMIN for reading nft/tc state
      # Dedicated system user (not DynamicUser) so /run/nifty-state is a real
      # directory readable by the dashboard's DynamicUser.
      User = "nifty-state";
      Group = "nifty-state";
      AmbientCapabilities = "CAP_NET_ADMIN";
      CapabilityBoundingSet = "CAP_NET_ADMIN";
      RuntimeDirectory = "nifty-state";
      RuntimeDirectoryPreserve = "yes";
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

      ExecStart = let
        dumpScript = pkgs.writeShellScript "nifty-state-dump" ''
          set -euo pipefail
          DIR=/run/nifty-state

          while true; do
            # Full nft ruleset as JSON (covers list chains + list chain + list table)
            nft -j list ruleset > "$DIR/nft-ruleset.json.tmp" && mv "$DIR/nft-ruleset.json.tmp" "$DIR/nft-ruleset.json"

            # Full nft ruleset as plain text (for per-chain rule listing)
            nft list ruleset > "$DIR/nft-ruleset.txt.tmp" && mv "$DIR/nft-ruleset.txt.tmp" "$DIR/nft-ruleset.txt"

            # ip addr as JSON
            ip -j addr show > "$DIR/ip-addr.json.tmp" && mv "$DIR/ip-addr.json.tmp" "$DIR/ip-addr.json"

            # tc state for all interfaces with qdiscs
            tc -s qdisc show > "$DIR/tc-qdisc.txt.tmp" 2>/dev/null && mv "$DIR/tc-qdisc.txt.tmp" "$DIR/tc-qdisc.txt" || true
            tc class show > "$DIR/tc-class.txt.tmp" 2>/dev/null && mv "$DIR/tc-class.txt.tmp" "$DIR/tc-class.txt" || true

            chmod 644 "$DIR"/*.json "$DIR"/*.txt 2>/dev/null || true
            sleep 3
          done
        '';
      in "${dumpScript}";
    };
  };

  # nifty-dashboard web UI
  # Binds to the management interface IP only.
  systemd.services.nifty-dashboard = mkIf cfg.packages.nifty-dashboard.enable {
    description = "nifty-dashboard web UI";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "nifty-filter.service" ];

    path = [ pkgs.systemd ];
    environment.ROOT_DIR = "/var/lib/private/nifty-dashboard";
    environment.SODOLA_STATE_FILE = "/run/nifty-filter/sodola-switch.json";
    environment.NIFTY_CONFIG_FILE = hclFile;
    environment.NIFTY_CONFIG_BOOT_SHA_FILE = "/run/nifty-filter/config-boot-sha";
    environment.NIFTY_STATE_DIR = "/run/nifty-state";
    serviceConfig = {
      Type = "simple";
      StateDirectory = "nifty-dashboard";
      ExecStart = "${pkgs.bash}/bin/bash -c 'MGMT_SUBNET=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} mgmt-subnet 2>/dev/null || true); MGMT_IP=$(echo \"$MGMT_SUBNET\" | cut -d/ -f1); [ -z \"$MGMT_IP\" ] && MGMT_IP=\"0.0.0.0\"; DASH_PORT=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} dashboard-port); exec ${nifty-dashboard}/bin/nifty-dashboard serve --net-listen-ip $MGMT_IP --net-listen-port $DASH_PORT'";
      Restart = "on-failure";
      RestartSec = "5s";

      # Privilege dropping — zero capabilities, all data comes from state dump files
      DynamicUser = true;
      CapabilityBoundingSet = "";

      # Filesystem hardening
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      ReadOnlyPaths = [
        hclFile
        "/run/nifty-filter"
        "/run/nifty-state"
        "/run/dnsmasq"
        "/var/lib/dnsmasq"
      ];

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
    };
  };
}
