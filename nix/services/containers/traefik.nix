# Traefik reverse proxy container
#
# TLS-terminating reverse proxy for infrastructure services.
# Uses host networking so it can bind ports 80/443 directly on the VM's IP.
#
# Static and dynamic configs are generated declaratively by Nix.
# A self-signed certificate is generated on first boot and persisted in
# /var/lib/traefik/certs/ so it can be pinned by clients.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
  traefikCfg = cfg.traefik;

  # Build the effective Traefik rule for a router, prepending ClientIP()
  # constraints when allowVlans is set.
  effectiveRule = name: r:
    let
      cidrs = map (v: traefikCfg.vlans.${v}) r.allowVlans;
      clientIp = "ClientIP(${lib.concatMapStringsSep ", " (c: "\`${c}\`") cidrs})";
    in
      if r.allowVlans == [] then r.rule
      else "${clientIp} && ${r.rule}";

  # Generate the dynamic file provider config (routers, services, TLS)
  dynamicConfig = {
    tls = {
      certificates = [
        {
          certFile = "/certs/traefik.crt";
          keyFile = "/certs/traefik.key";
        }
      ];
    };
    http = {
      routers = lib.mapAttrs (name: r: {
        rule = effectiveRule name r;
        service = name;
        entryPoints = r.entryPoints;
        tls = {};
      }) traefikCfg.routers;
      services = lib.mapAttrs (name: r: {
        loadBalancer.servers = [
          { url = r.backend; }
        ];
      }) traefikCfg.routers;
    };
  };

  dynamicConfigFile = pkgs.writeText "traefik-dynamic.yml"
    (builtins.toJSON dynamicConfig);

  # Static traefik config
  staticConfig = {
    entryPoints = {
      web = {
        address = ":80";
        http.redirections.entryPoint = {
          to = "websecure";
          scheme = "https";
        };
      };
      websecure = {
        address = ":443";
      };
    } // lib.optionalAttrs traefikCfg.dashboard.enable {
      traefik = {
        address = ":${toString traefikCfg.dashboard.port}";
      };
    };
    providers.file = {
      filename = "/etc/traefik/dynamic.yml";
      watch = false;
    };
    log.level = traefikCfg.logLevel;
  } // lib.optionalAttrs traefikCfg.dashboard.enable {
    api = {
      dashboard = true;
      insecure = true;
    };
  };

  staticConfigFile = pkgs.writeText "traefik.yml"
    (builtins.toJSON staticConfig);

  certDir = "/var/lib/traefik/certs";
in
{
  options.services.nifty-services.traefik = {
    enable = lib.mkEnableOption "Traefik reverse proxy container";

    image = lib.mkOption {
      type = lib.types.str;
      default = "docker.io/traefik:v3";
      description = "Container image for the Traefik reverse proxy.";
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [ "DEBUG" "INFO" "WARN" "ERROR" ];
      default = "INFO";
      description = "Traefik log level.";
    };

    dashboard = {
      enable = lib.mkEnableOption "Traefik dashboard (insecure, for local use only)";

      port = lib.mkOption {
        type = lib.types.port;
        default = 8080;
        description = "Port for the Traefik dashboard.";
      };
    };

    cert = {
      subject = lib.mkOption {
        type = lib.types.str;
        default = "/CN=infra-services";
        description = "Subject for the self-signed TLS certificate.";
      };

      days = lib.mkOption {
        type = lib.types.int;
        default = 3650;
        description = "Validity period in days for the self-signed certificate.";
      };

      san = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        example = [ "DNS:infra.lan" "IP:10.99.2.10" ];
        description = "Subject Alternative Names for the self-signed certificate.";
      };
    };

    vlans = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {};
      example = { trusted = "10.99.10.0/24"; iot = "10.99.20.0/24"; };
      description = "Map of VLAN names to their CIDR subnets, used by allowVlans.";
    };

    routers = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          rule = lib.mkOption {
            type = lib.types.str;
            description = "Traefik routing rule (e.g. Host(`dns.lan`), PathPrefix(`/`)).";
            example = "Host(`dns.lan`)";
          };

          backend = lib.mkOption {
            type = lib.types.str;
            description = "Backend URL for the service.";
            example = "http://127.0.0.1:5380";
          };

          entryPoints = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ "websecure" ];
            description = "Traefik entrypoints this router listens on.";
          };

          allowVlans = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [];
            example = [ "trusted" ];
            description = "VLAN names (from traefik.vlans) allowed to access this router. Empty means allow all.";
          };
        };
      });
      default = {};
      description = "Declarative Traefik routers and their backend services.";
    };
  };

  config = lib.mkIf (cfg.enable && traefikCfg.enable) {
    # Generate self-signed certificate on first boot, persist in /var
    systemd.services.traefik-selfsign-cert = {
      description = "Generate self-signed TLS certificate for Traefik";
      wantedBy = [ "multi-user.target" ];
      before = [ "podman-traefik.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.openssl ];
      script = let
        sanArg = lib.optionalString (traefikCfg.cert.san != [])
          "-addext 'subjectAltName=${lib.concatStringsSep "," traefikCfg.cert.san}'";
      in ''
        mkdir -p ${certDir}
        if [ ! -f ${certDir}/traefik.key ]; then
          echo "Generating self-signed certificate..."
          openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
            -keyout ${certDir}/traefik.key \
            -out ${certDir}/traefik.crt \
            -days ${toString traefikCfg.cert.days} \
            -nodes \
            -subj '${traefikCfg.cert.subject}' \
            ${sanArg}
          echo "Certificate generated at ${certDir}/"
        else
          echo "Certificate already exists, skipping generation."
        fi
      '';
    };

    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.traefik = {
      image = traefikCfg.image;
      volumes = [
        "${staticConfigFile}:/etc/traefik/traefik.yml:ro"
        "${dynamicConfigFile}:/etc/traefik/dynamic.yml:ro"
        "${certDir}:/certs:ro"
      ];
      extraOptions = [
        "--network=host"
      ];
      dependsOn = [];
    };

    # Ensure the cert service runs before traefik starts
    systemd.services.podman-traefik = {
      after = [ "traefik-selfsign-cert.service" ];
      requires = [ "traefik-selfsign-cert.service" ];
    };

    networking.firewall.allowedTCPPorts = [ 80 443 ]
      ++ lib.optional traefikCfg.dashboard.enable traefikCfg.dashboard.port;
  };
}
