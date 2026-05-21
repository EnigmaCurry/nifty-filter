# Step-CA private PKI module
#
# Runs Smallstep Step-CA as a podman container on a dedicated VM.
# Bootstraps the CA on first boot, issues client certificates for
# inter-service mTLS, and handles renewal via a daily timer.
#
# Host paths for operator access (SCP to other VMs):
#   /var/lib/step-ca/certs/root_ca.crt              — root CA cert
#   /var/lib/step-ca/client-certs/<name>/cert.pem    — client certs
#   /var/lib/step-ca/client-certs/<name>/key.pem     — client keys

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-step-ca;
  inherit (lib) mkEnableOption mkOption types mkIf concatStringsSep;

  # Nix-built container image — no registry pull needed.
  image = pkgs.dockerTools.streamLayeredImage {
    name = "nifty-step-ca";
    tag = "latest";
    contents = [
      pkgs.step-ca
      pkgs.step-cli
      pkgs.cacert
      pkgs.bash
      pkgs.coreutils
      pkgs.openssl
    ];
    config = {
      Entrypoint = [ "${pkgs.step-ca}/bin/step-ca" ];
      Env = [ "STEPPATH=/home/step" ];
    };
  };

  hostDataDir = "/var/lib/step-ca";
  hostCertsDir = "${hostDataDir}/certs";
  hostClientCertsDir = "${hostDataDir}/client-certs";

  # Common podman run prefix for step-cli commands against the volume
  stepRun = ''${pkgs.podman}/bin/podman run --rm \
    -v step-ca-data:/home/step \
    -e STEPPATH=/home/step \
    -e HOME=/home/step \
    --entrypoint ${pkgs.step-cli}/bin/step \
    nifty-step-ca:latest'';

  portStr = toString cfg.port;
in
{
  options.services.nifty-step-ca = {
    enable = mkEnableOption "Step-CA private certificate authority";

    port = mkOption {
      type = types.port;
      default = 9443;
      description = "HTTPS listen port for Step-CA.";
    };

    caName = mkOption {
      type = types.str;
      default = "Nifty CA";
      description = "Name for the CA (used in step ca init --name).";
    };

    domain = mkOption {
      type = types.str;
      default = "nifty.internal";
      description = "Base domain for client certificate common names.";
    };

    dnsNames = mkOption {
      type = types.listOf types.str;
      default = [ "localhost" ];
      example = [ "localhost" "10.99.2.3" ];
      description = "DNS names and IP addresses for the CA certificate SANs.";
    };

    provisioner = mkOption {
      type = types.str;
      default = "admin";
      description = "JWK provisioner name for certificate issuance.";
    };

    clientCerts = mkOption {
      type = types.listOf types.str;
      default = [ "dashboard" "service-monitor" "traefik" ];
      description = "CN prefixes for client certificates to auto-issue (prefixed with domain).";
    };
  };

  config = mkIf cfg.enable {
    # Load the Nix-built image into podman before anything else.
    systemd.services.load-step-ca-image = {
      description = "Load nifty-step-ca container image";
      wantedBy = [ "multi-user.target" ];
      before = [ "step-ca-bootstrap.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.bash}/bin/bash -c '${image} | ${pkgs.podman}/bin/podman load'";
      };
    };

    # Bootstrap the CA on first boot (idempotent).
    systemd.services.step-ca-bootstrap = {
      description = "Bootstrap Step-CA if not yet initialized";
      wantedBy = [ "multi-user.target" ];
      after = [ "load-step-ca-image.service" ];
      before = [ "podman-step-ca.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.podman pkgs.openssl ];
      script = let
        dnsFlags = concatStringsSep "," cfg.dnsNames;
      in ''
        set -euo pipefail
        mkdir -p ${hostCertsDir} ${hostClientCertsDir}

        # Ensure the named volume exists.
        podman volume exists step-ca-data || podman volume create step-ca-data
        MOUNT=$(podman volume inspect step-ca-data --format '{{.Mountpoint}}')

        # Skip if already initialized.
        if [ -f "$MOUNT/config/ca.json" ]; then
          echo "Step-CA already initialized, skipping bootstrap."
          # Ensure host copy of root CA exists.
          if [ ! -f ${hostCertsDir}/root_ca.crt ] && [ -f "$MOUNT/certs/root_ca.crt" ]; then
            cp "$MOUNT/certs/root_ca.crt" ${hostCertsDir}/root_ca.crt
          fi
          exit 0
        fi

        echo "Bootstrapping Step-CA..."

        # Generate a random password for the CA keys.
        mkdir -p "$MOUNT/secrets"
        openssl rand -base64 32 > "$MOUNT/secrets/password"
        chmod 600 "$MOUNT/secrets/password"

        # Initialize the CA.
        ${stepRun} ca init \
          --name="${cfg.caName}" \
          --provisioner="${cfg.provisioner}" \
          --dns="${dnsFlags}" \
          --address=":${portStr}" \
          --deployment-type=standalone \
          --password-file=/home/step/secrets/password \
          --acme

        # Copy root CA cert to host for operator access.
        cp "$MOUNT/certs/root_ca.crt" ${hostCertsDir}/root_ca.crt
        echo "Step-CA bootstrap complete. Root CA at ${hostCertsDir}/root_ca.crt"
      '';
    };

    # The main Step-CA container.
    virtualisation.oci-containers.backend = "podman";
    virtualisation.oci-containers.containers.step-ca = {
      image = "nifty-step-ca:latest";
      cmd = [ "/home/step/config/ca.json" "--password-file" "/home/step/secrets/password" ];
      volumes = [ "step-ca-data:/home/step" ];
      environment = {
        STEPPATH = "/home/step";
        HOME = "/home/step";
      };
      extraOptions = [ "--network=host" ];
      dependsOn = [];
    };

    # Explicit ordering for the container.
    systemd.services.podman-step-ca = {
      after = [ "step-ca-bootstrap.service" ];
      requires = [ "step-ca-bootstrap.service" ];
    };

    # Wait for Step-CA to become healthy before issuing certs.
    systemd.services.step-ca-wait-healthy = {
      description = "Wait for Step-CA to become healthy";
      after = [ "podman-step-ca.service" ];
      before = [ "step-ca-issue-client-certs.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.curl ];
      script = ''
        set -euo pipefail
        echo "Waiting for Step-CA to become healthy..."
        for i in $(seq 1 30); do
          if curl -sk https://127.0.0.1:${portStr}/health | grep -q '"status":"ok"'; then
            echo "Step-CA is healthy."
            exit 0
          fi
          echo "Attempt $i/30: not yet healthy, waiting 2s..."
          sleep 2
        done
        echo "ERROR: Step-CA did not become healthy in 60 seconds."
        exit 1
      '';
    };

    # Issue client certificates for inter-service mTLS.
    systemd.services.step-ca-issue-client-certs = {
      description = "Issue client certificates for mTLS";
      after = [ "step-ca-wait-healthy.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.podman pkgs.openssl pkgs.coreutils ];
      script = let
        certNames = cfg.clientCerts;
        issueOne = name: ''
          CN="${name}.${cfg.domain}"
          DIR="${hostClientCertsDir}/${name}"
          mkdir -p "$DIR"

          # Skip if cert exists and is valid for at least 30 days.
          if [ -f "$DIR/cert.pem" ] && [ -f "$DIR/key.pem" ]; then
            if openssl x509 -in "$DIR/cert.pem" -checkend 2592000 -noout 2>/dev/null; then
              echo "Client cert for $CN is still valid, skipping."
            else
              echo "Client cert for $CN is expiring soon, re-issuing..."
              rm -f "$DIR/cert.pem" "$DIR/key.pem"
            fi
          fi

          if [ ! -f "$DIR/cert.pem" ]; then
            echo "Issuing client cert for $CN..."
            MOUNT=$(podman volume inspect step-ca-data --format '{{.Mountpoint}}')
            mkdir -p "$MOUNT/client-certs"
            ${stepRun} ca certificate "$CN" \
              "/home/step/client-certs/${name}-cert.pem" \
              "/home/step/client-certs/${name}-key.pem" \
              --ca-url="https://127.0.0.1:${portStr}" \
              --root=/home/step/certs/root_ca.crt \
              --provisioner="${cfg.provisioner}" \
              --provisioner-password-file=/home/step/secrets/password \
              --not-after=8760h
            cp "$MOUNT/client-certs/${name}-cert.pem" "$DIR/cert.pem"
            cp "$MOUNT/client-certs/${name}-key.pem" "$DIR/key.pem"
            chmod 644 "$DIR/cert.pem"
            chmod 600 "$DIR/key.pem"
            echo "Issued client cert for $CN at $DIR/"
          fi
        '';
      in concatStringsSep "\n" (map issueOne certNames);
    };

    # Daily renewal timer.
    systemd.timers.step-ca-renew-client-certs = {
      description = "Daily client certificate renewal check";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
      };
    };

    systemd.services.step-ca-renew-client-certs = {
      description = "Renew client certificates approaching expiry";
      after = [ "podman-step-ca.service" ];
      serviceConfig.Type = "oneshot";
      path = [ pkgs.podman pkgs.openssl pkgs.coreutils ];
      script = let
        renewOne = name: ''
          DIR="${hostClientCertsDir}/${name}"
          if [ -f "$DIR/cert.pem" ] && [ -f "$DIR/key.pem" ]; then
            if ! openssl x509 -in "$DIR/cert.pem" -checkend 2592000 -noout 2>/dev/null; then
              echo "Renewing cert for ${name}.${cfg.domain}..."
              MOUNT=$(podman volume inspect step-ca-data --format '{{.Mountpoint}}')
              ${stepRun} ca renew \
                "/home/step/client-certs/${name}-cert.pem" \
                "/home/step/client-certs/${name}-key.pem" \
                --ca-url="https://127.0.0.1:${portStr}" \
                --root=/home/step/certs/root_ca.crt \
                --force
              cp "$MOUNT/client-certs/${name}-cert.pem" "$DIR/cert.pem"
              cp "$MOUNT/client-certs/${name}-key.pem" "$DIR/key.pem"
              chmod 644 "$DIR/cert.pem"
              chmod 600 "$DIR/key.pem"
              echo "Renewed cert for ${name}.${cfg.domain}"
            fi
          fi
        '';
      in concatStringsSep "\n" (map renewOne cfg.clientCerts);
    };

    # Open the CA port.
    networking.firewall.allowedTCPPorts = [ cfg.port ];
  };
}
