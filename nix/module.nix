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

  nifty-filter = self.packages.${pkgs.system}.nifty-filter;

  configDir = "/var/nifty-filter";
  envFile = "${configDir}/nifty-filter.env";

in
{
  options.services.nifty-filter = {
    enable = mkEnableOption "nifty-filter nftables firewall";

    configPath = mkOption {
      type = types.str;
      default = envFile;
      description = "Path to the nifty-filter.env configuration file on the writable partition.";
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

    # Make the binary available system-wide
    environment.systemPackages = [ nifty-filter ];

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
  };
}
