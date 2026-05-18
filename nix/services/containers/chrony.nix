# Chrony NTP server container
#
# Serves NTP (UDP 123) to LAN clients.
# Uses host networking so clients can reach it directly on the VM's IP.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
in
{
  options.services.nifty-services.chrony = {
    enable = lib.mkEnableOption "chrony NTP server container";

    image = lib.mkOption {
      type = lib.types.str;
      default = "docker.io/cturra/ntp:latest";
      description = "Container image for the chrony NTP server.";
    };

    ntpServers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "time.cloudflare.com" "pool.ntp.org" ];
      description = "Upstream NTP servers for chrony to sync from.";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.chrony.enable) {
    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.chrony = {
      image = cfg.chrony.image;
      environment = {
        NTP_SERVERS = lib.concatStringsSep "," cfg.chrony.ntpServers;
      };
      extraOptions = [
        "--network=host"
        "--cap-add=SYS_TIME"
      ];
    };

    networking.firewall.allowedUDPPorts = [ 123 ];
  };
}
