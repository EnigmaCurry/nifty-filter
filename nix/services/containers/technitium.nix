# Technitium DNS server container (stateless)
#
# Serves as the authoritative/recursive DNS server for the LAN.
# dnsmasq on the router forwards DNS queries here via dns.upstream in HCL.
# Uses host networking so clients can reach it directly on the VM's IP.
# Web admin UI listens on localhost:5380 only (accessed through Traefik).
#
# Technitium runs without a persistent volume — all state is ephemeral.
# nifty-service-monitor declaratively manages zones, records, and forwarders
# from the HCL config on every poll cycle. Any zones not in the config
# (and not in unmanaged_zones) are deleted.
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

    # Generate the admin password file before Technitium starts.
    # The file lives on the service-monitor-data volume so both the
    # Technitium container (via DNS_SERVER_ADMIN_PASSWORD_FILE) and the
    # service-monitor container share the same credential.
    systemd.services.technitium-admin-password = {
      description = "Ensure Technitium admin password file exists";
      wantedBy = [ "multi-user.target" ];
      before = [ "podman-technitium.service" ];
      after = [ "podman-volumes.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.podman ];
      script = ''
        # Ensure the named volume exists.
        podman volume exists service-monitor-data || podman volume create service-monitor-data
        MOUNT=$(podman volume inspect service-monitor-data --format '{{.Mountpoint}}')
        FILE="$MOUNT/technitium-admin-password"
        if [ ! -s "$FILE" ]; then
          ${pkgs.openssl}/bin/openssl rand -hex 16 > "$FILE"
          chmod 0600 "$FILE"
          echo "Generated new Technitium admin password"
        else
          echo "Technitium admin password already exists"
        fi
      '';
    };

    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.technitium = {
      image = cfg.technitium.image;
      environment = {
        # Bind web UI to localhost only — access it through Traefik
        DNS_SERVER_WEB_SERVICE_LOCAL_ADDRESSES = "127.0.0.1,[::1]";
        # Seed admin password from the shared volume so the container
        # never starts with the default "admin" password.
        DNS_SERVER_ADMIN_PASSWORD_FILE = "/data/technitium-admin-password";
      };
      volumes = [
        "service-monitor-data:/data"
      ];
      extraOptions = [
        "--network=host"
      ];
    };

    # Traefik routing for Technitium is managed dynamically by the
    # service-monitor, which writes Host rules based on the HCL domain.
    # See service-monitor's traefik config writing in technitium.rs.

    networking.firewall.allowedTCPPorts = [ 53 ];
    networking.firewall.allowedUDPPorts = [ 53 ];
  };
}
