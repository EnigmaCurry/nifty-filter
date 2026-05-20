# Service monitor container
#
# Polls the router API for services config and declaratively applies it
# to infrastructure services (Technitium DNS zones, records, forwarders,
# user accounts, etc.).
#
# Runs as a podman container with host networking so it can reach both
# the router API and Technitium on localhost:5380.
# A named volume persists the TOFU certificate pin and admin password.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;

  # Build a minimal container image from the Nix package.
  image = pkgs.dockerTools.streamLayeredImage {
    name = "nifty-service-monitor";
    tag = "latest";
    contents = [ cfg.service-monitor.package pkgs.cacert ];
    config = {
      Entrypoint = [ "${cfg.service-monitor.package}/bin/nifty-service-monitor" ];
    };
  };
in
{
  options.services.nifty-services.service-monitor = {
    enable = lib.mkEnableOption "nifty service configuration monitor";

    routerUrl = lib.mkOption {
      type = lib.types.str;
      description = "Base URL of the router's nifty-dashboard API (e.g. https://10.99.2.1)";
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
    # Load the Nix-built image into podman before the container starts.
    systemd.services.load-service-monitor-image = {
      description = "Load nifty-service-monitor container image";
      wantedBy = [ "multi-user.target" ];
      before = [ "podman-service-monitor.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -c '${image} | ${pkgs.podman}/bin/podman load'";
      };
    };

    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.service-monitor = {
      image = "nifty-service-monitor:latest";
      environment = {
        RUST_LOG = "info";
      };
      cmd = [
        "--router-url" cfg.service-monitor.routerUrl
        "--poll-interval" (toString cfg.service-monitor.pollInterval)
        "--state-dir" "/data"
        "--traefik-dynamic-dir" "/traefik-dynamic"
      ];
      volumes = [
        "service-monitor-data:/data"
        "traefik-dynamic:/traefik-dynamic"
      ];
      extraOptions = [
        "--network=host"
      ];
      dependsOn = [];
    };

    # Soft dependency: start after technitium but don't fail if it's slow.
    # The service-monitor polls and handles technitium being unavailable.
    systemd.services.podman-service-monitor = {
      after = [ "podman-technitium.service" ];
    };
  };
}
