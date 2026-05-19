# Technitium DNS server container
#
# Serves as the authoritative/recursive DNS server for the LAN.
# dnsmasq on the router forwards DNS queries here via dns.upstream in HCL.
# Uses host networking so clients can reach it directly on the VM's IP.
# Web admin UI listens on localhost:5380 only (accessed through Traefik).
#
# The VM's resolv.conf points at the router (bootstrapDns) so that container
# image pulls work even before Technitium is running. The router's dnsmasq
# should have a fallback upstream (e.g. 1.1.1.1) for this to work.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
in
{
  options.services.nifty-services.technitium = {
    enable = lib.mkEnableOption "Technitium DNS server container";

    image = lib.mkOption {
      type = lib.types.str;
      default = "docker.io/technitium/dns-server:latest";
      description = "Container image for the Technitium DNS server.";
    };

    webPort = lib.mkOption {
      type = lib.types.port;
      default = 5380;
      description = "Port for the Technitium web admin interface.";
    };

    bootstrapDns = lib.mkOption {
      type = lib.types.str;
      default = "10.99.2.1";
      description = "Bootstrap DNS server for image pulls before Technitium is running (typically the router).";
    };
  };

  config = lib.mkIf (cfg.enable && cfg.technitium.enable) {
    # Disable systemd-resolved so port 53 is available for Technitium.
    # dns-identity.nix in nixos-vm-template uses mkForce on the whole
    # resolv.conf attrset, so we must use mkOverride 10 on it as well.
    services.resolved.enable = lib.mkOverride 10 false;
    networking.resolvconf.enable = lib.mkOverride 10 false;
    environment.etc."resolv.conf" = lib.mkOverride 10 {
      source = pkgs.writeText "resolv.conf" "nameserver ${cfg.technitium.bootstrapDns}\n";
      mode = "0644";
    };

    # On read-only root, /etc is an overlay and the normal etc activation
    # may not create resolv.conf. Write it directly as a fallback.
    system.activationScripts.resolvConf = lib.stringAfter [ "etc" ] ''
      if [ ! -e /etc/resolv.conf ]; then
        cp ${pkgs.writeText "resolv.conf" "nameserver ${cfg.technitium.bootstrapDns}\n"} /etc/resolv.conf
        chmod 0644 /etc/resolv.conf
      fi
    '';

    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.technitium = {
      image = cfg.technitium.image;
      environment = {
        # Bind web UI to localhost only — access it through Traefik
        DNS_SERVER_WEB_SERVICE_LOCAL_ADDRESSES = "127.0.0.1,[::1]";
      };
      volumes = [
        "technitium-data:/etc/dns"
      ];
      extraOptions = [
        "--network=host"
      ];
    };

    networking.firewall.allowedTCPPorts = [ 53 ];
    networking.firewall.allowedUDPPorts = [ 53 ];
  };
}
