# Service monitor: polls router API for services config and applies it
# to infrastructure services (Technitium password, etc.).
#
# This is a native systemd service (not a container) that runs alongside
# the containerised infrastructure services on the services VM.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
in
{
  options.services.nifty-services.service-monitor = {
    enable = lib.mkEnableOption "nifty service configuration monitor";

    routerUrl = lib.mkOption {
      type = lib.types.str;
      description = "Base URL of the router's nifty-dashboard API (e.g. http://10.99.2.1:3000)";
    };

    pollInterval = lib.mkOption {
      type = lib.types.int;
      default = 15;
      description = "Polling interval in seconds";
    };

    package = lib.mkOption {
      type = lib.types.package;
      description = "The nifty-service-monitor package";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.service-monitor.enable) {
    systemd.services.nifty-service-monitor = {
      description = "Nifty service configuration monitor";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" "podman-technitium.service" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.concatStringsSep " " [
          "${cfg.service-monitor.package}/bin/nifty-service-monitor"
          "--router-url" cfg.service-monitor.routerUrl
          "--poll-interval" (toString cfg.service-monitor.pollInterval)
        ];
        DynamicUser = true;
        StateDirectory = "nifty-service-monitor";
        Restart = "always";
        RestartSec = 5;

        # Security hardening
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
      };

      environment = {
        RUST_LOG = "info";
      };
    };
  };
}
