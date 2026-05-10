# NixOS module for nifty-filter (read-only system model)
#
# The system root is read-only. Configuration lives as an HCL file
# on the writable /var partition:
#
#   /var/nifty-filter/nifty-filter.hcl
#
# Systemd services read this file at boot and apply network + firewall config.
# To reconfigure the router: edit the HCL file and reboot.
self:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-filter;
  inherit (lib) mkEnableOption mkOption types mkIf;

  nifty-filter = self.packages.${pkgs.stdenv.hostPlatform.system}.nifty-filter;

  configDir = "/var/nifty-filter";
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
      unitConfig.ConditionPathExists = "!${hclFile}";
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        mkdir -p ${configDir}
        cp ${./default-nifty-filter.hcl} ${hclFile}
        chmod 0600 ${hclFile}
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

    # Set hostname from HCL config at boot
    systemd.services.nifty-hostname = {
      description = "Set hostname from nifty-filter HCL config";
      wantedBy = [ "sysinit.target" ];
      before = [ "network-pre.target" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.hostname ];
      script = ''
        if [ -f ${hclFile} ]; then
          NAME=$(${nifty-filter}/bin/nifty-filter hostname --config ${hclFile} 2>/dev/null)
          if [ -n "$NAME" ]; then
            hostname "$NAME"
          fi
        fi
      '';
    };

    # Generate interface rename rules (.link files) from HCL config at boot
    systemd.services.nifty-link = {
      description = "Generate interface rename rules from HCL config";
      wantedBy = [ "sysinit.target" ];
      before = [ "systemd-udevd.service" "systemd-networkd.service" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        if [ ! -f ${hclFile} ]; then
          echo "No HCL config found, skipping link generation"
          exit 0
        fi
        mkdir -p /run/systemd/network
        ${nifty-filter}/bin/nifty-filter generate linkfiles --config ${hclFile} --output-dir /run/systemd/network
      '';
    };

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

        # Read interface names from HCL
        WAN_INTERFACE=$(${pkgs.gnugrep}/bin/grep -oP 'wan\s*=\s*"\K[^"]+' ${hclFile} | head -1)
        TRUNK_INTERFACE=$(${pkgs.gnugrep}/bin/grep -oP 'trunk\s*=\s*"\K[^"]+' ${hclFile} | head -1)
        MGMT_INTERFACE=$(${pkgs.gnugrep}/bin/grep -oP 'mgmt\s*=\s*"\K[^"]+' ${hclFile} | head -1)
        ENABLE_IPV6=$(${pkgs.gnugrep}/bin/grep -oP 'enable_ipv6\s*=\s*\K\w+' ${hclFile} | head -1)

        # Bring up interfaces
        [ -n "$WAN_INTERFACE" ] && ip link set "$WAN_INTERFACE" up
        [ -n "$TRUNK_INTERFACE" ] && ip link set "$TRUNK_INTERFACE" up
        [ -n "$MGMT_INTERFACE" ] && ip link set "$MGMT_INTERFACE" up

        # Generate networkd config files
        mkdir -p /run/systemd/network
        ${nifty-filter}/bin/nifty-filter generate networkd --config ${hclFile} --output-dir /run/systemd/network

        # Ensure WAN accepts RAs despite forwarding (must override after networkd)
        if [ "$ENABLE_IPV6" = "true" ] && [ -n "$WAN_INTERFACE" ]; then
          mkdir -p /etc/systemd/system/systemd-networkd.service.d
          cat > /etc/systemd/system/systemd-networkd.service.d/accept-ra.conf <<RAEOF
        [Service]
        ExecStartPost=/bin/sh -c 'sleep 1 && /run/current-system/sw/bin/sysctl -w net.ipv6.conf.$WAN_INTERFACE.accept_ra=2 net.ipv6.conf.$WAN_INTERFACE.forwarding=0'
        RAEOF
          systemctl daemon-reload
        fi

        # Disable IPv6 on management interface
        if [ -n "$MGMT_INTERFACE" ]; then
          sysctl -w net.ipv6.conf.$MGMT_INTERFACE.disable_ipv6=1
        fi

        # Restart networkd to pick up the new configs
        networkctl reload || systemctl restart systemd-networkd
      '';
    };

    # dnsmasq: DHCP + DNS for LAN
    systemd.services.nifty-dnsmasq = {
      description = "dnsmasq DHCP and DNS server";
      wantedBy = [ "multi-user.target" ];
      after = [ "nifty-network.service" ];
      serviceConfig = {
        Type = "forking";
        PIDFile = "/run/dnsmasq.pid";
        ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq -C /run/dnsmasq.conf";
        ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
        Restart = "on-failure";
      };
      preStart = ''
        mkdir -p /var/lib/dnsmasq
        if [ -f ${hclFile} ]; then
          ${nifty-filter}/bin/nifty-filter generate dnsmasq --config ${hclFile} --output /run/dnsmasq.conf
        else
          ${nifty-filter}/bin/nifty-filter generate dnsmasq-minimal --output /run/dnsmasq.conf
        fi
      '';
    };

    # Apply nftables rules from the HCL config at every boot
    systemd.services.nifty-filter = {
      description = "Apply nifty-filter nftables ruleset";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-pre.target" "nifty-filter-init.service" ];
      before = [ "network.target" ];
      wants = [ "network-pre.target" ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -c '${nifty-filter}/bin/nifty-filter nftables --config ${hclFile} | ${pkgs.nftables}/bin/nft -f -'";
        ExecStop = "${pkgs.nftables}/bin/nft flush ruleset";
      };

      preStart = ''
        if [ ! -f ${hclFile} ]; then
          echo "ERROR: ${hclFile} not found. Applying emergency lockdown rules."
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
      '';
    };

    # QoS traffic shaping (CAKE qdisc) — runs only when qos block is configured
    systemd.services.nifty-qos = {
      description = "Apply QoS traffic shaping (CAKE)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      path = [ pkgs.iproute2 pkgs.kmod pkgs.bash ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -o pipefail -c '${nifty-filter}/bin/nifty-filter qos --config ${hclFile} | ${pkgs.bash}/bin/bash'";
        ExecStop = "${pkgs.bash}/bin/bash -c 'WAN_IFACE=$(${nifty-filter}/bin/nifty-filter hostname --config ${hclFile} 2>/dev/null; ${pkgs.coreutils}/bin/cat ${hclFile} | ${pkgs.gnugrep}/bin/grep -oP \"wan\\s*=\\s*\\\"\\K[^\\\"]+\" | head -1); ${pkgs.iproute2}/bin/tc qdisc del dev \"$WAN_IFACE\" root 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev \"$WAN_IFACE\" ingress 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev ifb0 root 2>/dev/null; ${pkgs.iproute2}/bin/ip link set ifb0 down 2>/dev/null; true'";
      };
    };

    # iperf3 bandwidth testing server (enabled via packages.iperf.enable)
    systemd.services.nifty-iperf = mkIf cfg.packages.iperf.enable {
      description = "iperf3 bandwidth testing server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" "nifty-filter.service" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${pkgs.bash}/bin/bash -c 'PORT=$(${pkgs.gnugrep}/bin/grep -oP \"iperf_port\\s*=\\s*\\K[0-9]+\" ${hclFile} 2>/dev/null || echo 5201); exec ${pkgs.iperf3}/bin/iperf3 --server --port $PORT'";
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
        StateDirectory = "nifty-dashboard";
        ExecStart = "${pkgs.bash}/bin/bash -c 'MGMT_IP=$(${pkgs.gnugrep}/bin/grep -oP \"mgmt_subnet\\s*=\\s*\\\"\\K[^\\\"/]+\" ${hclFile} 2>/dev/null || echo \"0.0.0.0\"); DASH_PORT=$(${pkgs.gnugrep}/bin/grep -oP \"dashboard_port\\s*=\\s*\\K[0-9]+\" ${hclFile} 2>/dev/null || echo 3000); exec ${nifty-dashboard}/bin/nifty-dashboard serve --net-listen-ip $MGMT_IP --net-listen-port $DASH_PORT'";
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
        ExecStartPre = [
          # Block forwarded traffic to the switch management subnet —
          # only the router itself (output chain) may reach the switch.
          "${pkgs.bash}/bin/bash -c 'ROUTER_IP=$(${pkgs.gnugrep}/bin/grep -oP \"router_ip\\s*=\\s*\\\"\\K[^\\\"]+\" ${hclFile} 2>/dev/null); if [ -n \"$ROUTER_IP\" ]; then NETWORK=$(echo $ROUTER_IP | ${pkgs.gnused}/bin/sed \"s|\\.[0-9]*/|.0/|\"); nft insert rule inet filter forward ip daddr $NETWORK drop comment \"block switch mgmt\" 2>/dev/null || true; fi'"
        ];
        ExecStart = "${sodola-switch}/bin/sodola-switch supervise --interval 60 --config ${hclFile} --state-file /run/nifty-filter/sodola-switch.json";
        Restart = "on-failure";
        RestartSec = "10s";
      };
    };
  };
}
