# DDNS updater container (stateless)
#
# Keeps DNS A/AAAA records updated at external providers (DuckDNS, Cloudflare, etc.)
# when the WAN IP changes.
#
# Runs without a persistent volume — configuration is written to a shared volume
# by the service-monitor on each poll cycle. A systemd path unit watches the
# config file and restarts the container when it changes.
#
# Image: ghcr.io/qdm12/ddns-updater

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
  configPath = "/var/lib/ddns-updater/config.json";
in
{
  options.services.nifty-services.ddns = {
    enable = lib.mkEnableOption "DDNS updater container";

    image = lib.mkOption {
      type = lib.types.str;
      default = "ghcr.io/qdm12/ddns-updater";
      description = "Container image for the DDNS updater.";
    };

    period = lib.mkOption {
      type = lib.types.str;
      default = "5m";
      description = "How often to check the public IP for changes.";
    };

    publicipFetchers = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "http";
      description = "Comma-separated fetcher types to obtain the public IP (http, dns). Default: all.";
    };

    publicipHttpProviders = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Comma-separated HTTP providers for public IP detection (ipv4 or ipv6). Default: all.";
    };

    publicipv4HttpProviders = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Comma-separated HTTP providers for public IPv4 detection only. Default: all.";
    };

    publicipv6HttpProviders = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Comma-separated HTTP providers for public IPv6 detection only. Default: all.";
    };

    updateCooldownPeriod = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "5m";
      description = "Cooldown duration between updates for each record. Default: 5m.";
    };

    httpTimeout = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "10s";
      description = "Timeout for all HTTP requests. Default: 10s.";
    };

    serverEnabled = lib.mkOption {
      type = lib.types.nullOr lib.types.bool;
      default = null;
      description = "Enable the web UI server. Default: true.";
    };

    shoutrrrAddresses = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Comma-separated Shoutrrr notification addresses (Telegram, Discord, etc.).";
    };

    tz = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "America/New_York";
      description = "Timezone for log timestamps.";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.ddns.enable) {
    # Ensure the config directory exists before the container starts.
    # The service-monitor writes config.json here; on first boot the file
    # may not exist yet, so we seed an empty config.
    systemd.services.ddns-updater-init = {
      description = "Seed DDNS updater config directory";
      wantedBy = [ "multi-user.target" ];
      before = [ "podman-ddns-updater.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        mkdir -p "$(dirname ${configPath})"
        if [ ! -f "${configPath}" ]; then
          echo '{"settings":[]}' > "${configPath}"
        fi
      '';
    };

    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.ddns-updater = {
      image = cfg.ddns.image;
      environment = {
        PERIOD = cfg.ddns.period;
        LISTENING_ADDRESS = ":8000";
        LOG_LEVEL = "info";
      } // lib.optionalAttrs (cfg.ddns.publicipFetchers != null) {
        PUBLICIP_FETCHERS = cfg.ddns.publicipFetchers;
      } // lib.optionalAttrs (cfg.ddns.publicipHttpProviders != null) {
        PUBLICIP_HTTP_PROVIDERS = cfg.ddns.publicipHttpProviders;
      } // lib.optionalAttrs (cfg.ddns.publicipv4HttpProviders != null) {
        PUBLICIPV4_HTTP_PROVIDERS = cfg.ddns.publicipv4HttpProviders;
      } // lib.optionalAttrs (cfg.ddns.publicipv6HttpProviders != null) {
        PUBLICIPV6_HTTP_PROVIDERS = cfg.ddns.publicipv6HttpProviders;
      } // lib.optionalAttrs (cfg.ddns.updateCooldownPeriod != null) {
        UPDATE_COOLDOWN_PERIOD = cfg.ddns.updateCooldownPeriod;
      } // lib.optionalAttrs (cfg.ddns.httpTimeout != null) {
        HTTP_TIMEOUT = cfg.ddns.httpTimeout;
      } // lib.optionalAttrs (cfg.ddns.serverEnabled != null) {
        SERVER_ENABLED = if cfg.ddns.serverEnabled then "yes" else "no";
      } // lib.optionalAttrs (cfg.ddns.shoutrrrAddresses != null) {
        SHOUTRRR_ADDRESSES = cfg.ddns.shoutrrrAddresses;
      } // lib.optionalAttrs (cfg.ddns.tz != null) {
        TZ = cfg.ddns.tz;
      };
      volumes = [
        "${configPath}:/updater/data/config.json"
      ];
      extraOptions = [
        "--network=host"
      ];
    };

    systemd.services.podman-ddns-updater = {
      after = [ "ddns-updater-init.service" ];
      requires = [ "ddns-updater-init.service" ];
    };

    # Watch the config file and restart the container when it changes.
    systemd.paths.ddns-updater-config-watcher = {
      wantedBy = [ "multi-user.target" ];
      pathConfig = {
        PathChanged = configPath;
      };
    };

    systemd.services.ddns-updater-config-watcher = {
      description = "Restart ddns-updater on config change";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${pkgs.systemd}/bin/systemctl restart podman-ddns-updater.service";
      };
    };
  };
}
