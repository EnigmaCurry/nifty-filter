# NixOS module for nifty-filter (read-only system model)
#
# The system root is read-only. Configuration lives as an env file
# on the writable /var partition:
#
#   /var/nifty-filter/nifty-filter.env
#
# A systemd service reads this file at boot and applies the nftables ruleset.
# To reconfigure the router: edit the env file and reboot.
self:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-filter;
  inherit (lib) mkEnableOption mkOption types mkIf;

  nifty-filter = self.packages.${pkgs.stdenv.hostPlatform.system}.nifty-filter;

  configDir = "/var/nifty-filter";
  envFile = "${configDir}/nifty-filter.env";
  hclFile = "${configDir}/nifty-filter.hcl";

  sodola-switch = self.packages.${pkgs.stdenv.hostPlatform.system}.sodola-switch;
  sodolaConfigDir = "${configDir}/sodola-switch";
  sodola-credentials = "${sodolaConfigDir}/credentials";

  nifty-dashboard = self.packages.${pkgs.stdenv.hostPlatform.system}.nifty-dashboard;

  # Collect enabled optional packages
  optionalPackages = lib.concatLists [
    (lib.optional cfg.packages.sodola-switch.enable sodola-switch)
    (lib.optional cfg.packages.nifty-dashboard.enable nifty-dashboard)
    (lib.optional cfg.packages.iperf.enable pkgs.iperf3)
  ];

in
{
  options.services.nifty-filter = {
    enable = mkEnableOption "nifty-filter nftables firewall";

    configPath = mkOption {
      type = types.str;
      default = envFile;
      description = "Path to the nifty-filter.env configuration file on the writable partition.";
    };

    packages = {
      sodola-switch = {
        enable = mkEnableOption "Sodola SL-SWTGW218AS managed switch client";
      };
      nifty-dashboard = {
        enable = mkEnableOption "nifty-dashboard web UI";
      };
      iperf = {
        enable = mkEnableOption "iperf3 bandwidth testing server";
      };
    };
  };

  config = mkIf cfg.enable {
    # Disable NixOS's built-in firewall (we replace it entirely)
    networking.firewall.enable = false;

    # Enable nftables (we manage the ruleset ourselves via the boot service)
    networking.nftables.enable = true;

    # IP forwarding (it's a router)
    # IPv6 forwarding is set per-interface (not all.forwarding) so that
    # the WAN interface can still accept Router Advertisements.
    # all.forwarding=1 forces accept_ra=0 on every interface, breaking DHCPv6-PD.
    boot.kernel.sysctl = {
      "net.ipv4.ip_forward" = 1;
      "net.ipv6.conf.default.forwarding" = 1;
    };

    # Kernel modules for QoS traffic shaping (CAKE qdisc + IFB for download)
    boot.kernelModules = [ "ifb" "sch_cake" ];

    # Make the binary available system-wide
    environment.systemPackages = [ nifty-filter ] ++ optionalPackages;

    # Seed the default config on first boot if it doesn't exist
    systemd.services.nifty-filter-init = {
      description = "Initialize nifty-filter default configuration";
      wantedBy = [ "multi-user.target" ];
      before = [ "nifty-filter.service" ];
      unitConfig.ConditionPathExists = "!${cfg.configPath}";
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        mkdir -p ${configDir}
        cp ${./default-nifty-filter.env} ${cfg.configPath}
        chmod 0600 ${cfg.configPath}
        mkdir -p ${configDir}/ssh
      '';
    };

    # Snapshot the config file SHA at boot so the dashboard can detect drift
    systemd.services.nifty-config-sha = {
      description = "Record config SHA256 at boot";
      wantedBy = [ "multi-user.target" ];
      after = [ "nifty-filter-init.service" ];
      before = [ "nifty-filter.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        RuntimeDirectory = "nifty-filter";
        RuntimeDirectoryPreserve = "yes";
      };
      script = ''
        if [ -f ${hclFile} ]; then
          ${pkgs.coreutils}/bin/sha256sum ${hclFile} \
            | ${pkgs.coreutils}/bin/cut -d' ' -f1 \
            > /run/nifty-filter/config-boot-sha
          ${pkgs.coreutils}/bin/cp ${hclFile} /run/nifty-filter/config-boot-snapshot
        else
          echo "" > /run/nifty-filter/config-boot-sha
          echo "" > /run/nifty-filter/config-boot-snapshot
        fi
        ${pkgs.coreutils}/bin/chmod 0444 /run/nifty-filter/config-boot-sha /run/nifty-filter/config-boot-snapshot
      '';
    };

    # Apply nftables rules from the env file at every boot
    systemd.services.nifty-filter = {
      description = "Apply nifty-filter nftables ruleset";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-pre.target" "nifty-filter-init.service" ];
      before = [ "network.target" ];
      wants = [ "network-pre.target" ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -c '${nifty-filter}/bin/nifty-filter nftables --env-file ${cfg.configPath} --strict-env | ${pkgs.nftables}/bin/nft -f -'";
        ExecStop = "${pkgs.nftables}/bin/nft flush ruleset";
      };

      preStart = ''
        if [ ! -f ${cfg.configPath} ]; then
          echo "ERROR: ${cfg.configPath} not found. Applying emergency lockdown rules."
          ${pkgs.nftables}/bin/nft -f - <<'LOCKDOWN'
        flush ruleset
        table inet filter {
          chain input {
            type filter hook input priority 0; policy drop;
            ct state established,related accept
            iif "lo" accept
          }
          chain forward {
            type filter hook forward priority 0; policy drop;
          }
          chain output {
            type filter hook output priority 0; policy drop;
            oif "lo" accept
          }
        }
        LOCKDOWN
          exit 1
        fi

        # Check ENABLED flag
        ENABLED=$(${pkgs.gnugrep}/bin/grep -oP '^ENABLED=\K.*' ${cfg.configPath} | ${pkgs.gnused}/bin/sed "s/^\([\"']\)\(.*\)\1$/\2/" || echo "false")
        if [ "$ENABLED" != "true" ]; then
          echo ""
          echo "============================================"
          echo " nifty-filter is not enabled."
          echo ""
          echo " Configure your router:"
          echo "   sudo nano ${cfg.configPath}"
          echo ""
          echo " Set your interfaces (ip link to identify),"
          echo " then set ENABLED=true and reboot."
          echo "============================================"
          echo ""
          exit 1
        fi
      '';
    };

    # QoS traffic shaping (CAKE qdisc) — runs only when WAN_QOS_*_MBPS are set
    systemd.services.nifty-qos = {
      description = "Apply QoS traffic shaping (CAKE)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      path = [ pkgs.iproute2 pkgs.kmod pkgs.bash ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -o pipefail -c '${nifty-filter}/bin/nifty-filter qos --env-file ${cfg.configPath} --strict-env | ${pkgs.bash}/bin/bash'";
        ExecStop = "${pkgs.bash}/bin/bash -c 'IFACE=$(${pkgs.gnugrep}/bin/grep -oP \"^WAN_INTERFACE=\\K.*\" ${cfg.configPath} | ${pkgs.coreutils}/bin/tr -d \"\\047\\042\"); ${pkgs.iproute2}/bin/tc qdisc del dev \"$IFACE\" root 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev \"$IFACE\" ingress 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev ifb0 root 2>/dev/null; ${pkgs.iproute2}/bin/ip link set ifb0 down 2>/dev/null; true'";
      };
    };

    # iperf3 bandwidth testing server (enabled via packages.iperf.enable)
    systemd.services.nifty-iperf = mkIf cfg.packages.iperf.enable {
      description = "iperf3 bandwidth testing server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      serviceConfig = {
        Type = "simple";
        EnvironmentFile = cfg.configPath;
        ExecStart = "${pkgs.bash}/bin/bash -c '${pkgs.iperf3}/bin/iperf3 --server --port \${IPERF_PORT:-5201}'";
        Restart = "on-failure";
        RestartSec = "5s";
        DynamicUser = true;
      };
    };

    # nifty-dashboard web UI (enabled via packages.nifty-dashboard.enable)
    # Binds to the management interface IP only.
    systemd.services.nifty-dashboard = mkIf cfg.packages.nifty-dashboard.enable {
      description = "nifty-dashboard web UI";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      path = [ pkgs.iproute2 pkgs.nftables ];
      environment.ROOT_DIR = "/var/lib/nifty-dashboard";
      environment.SODOLA_STATE_FILE = "/run/nifty-filter/sodola-switch.json";
      environment.NIFTY_CONFIG_FILE = hclFile;
      environment.NIFTY_CONFIG_BOOT_SHA_FILE = "/run/nifty-filter/config-boot-sha";
      serviceConfig = {
        Type = "simple";
        EnvironmentFile = cfg.configPath;
        StateDirectory = "nifty-dashboard";
        ExecStart = "${pkgs.bash}/bin/bash -c '${nifty-dashboard}/bin/nifty-dashboard serve --net-listen-ip $(echo $MGMT_SUBNET | cut -d/ -f1) --net-listen-port \${NIFTY_DASHBOARD_PORT:-3000}'";
        Restart = "on-failure";
        RestartSec = "5s";
      };
    };

    # Sodola switch supervisor (enabled via packages.sodola-switch.enable)
    systemd.services.nifty-sodola-switch = mkIf cfg.packages.sodola-switch.enable {
      description = "Supervise Sodola switch VLAN configuration";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      path = [ pkgs.iproute2 pkgs.nftables ];
      serviceConfig = {
        Type = "simple";
        RuntimeDirectory = "nifty-filter";
        RuntimeDirectoryPreserve = "yes";
        EnvironmentFile = cfg.configPath;
        ExecStartPre = [
          # Block forwarded traffic to the switch management subnet —
          # only the router itself (output chain) may reach the switch.
          "${pkgs.bash}/bin/bash -c 'NETWORK=$(echo $SODOLA_ROUTER_IP | sed \"s|\\.[0-9]*/|.0/|\"); nft insert rule inet filter forward ip daddr $NETWORK drop comment \"block switch mgmt\" 2>/dev/null || true'"
        ];
        ExecStart = "${sodola-switch}/bin/sodola-switch supervise --interval 60 --env-file ${cfg.configPath} --state-file /run/nifty-filter/sodola-switch.json";
        Restart = "on-failure";
        RestartSec = "10s";
      };
    };
  };
}
