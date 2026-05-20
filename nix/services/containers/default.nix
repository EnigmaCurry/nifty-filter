# NixOS module for nifty infrastructure services (containerized via podman)
#
# These services are designed to run on a separate VM from the router,
# deployed via nixos-vm-template with the podman + nifty-services profiles.

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-services;
  inherit (lib) mkEnableOption mkOption types;
in
{
  imports = [
    ./chrony.nix
    ./technitium.nix
    ./traefik.nix
    ./service-monitor.nix
  ];

  options.services.nifty-services = {
    enable = mkEnableOption "nifty infrastructure services (containerized)";

    domain = mkOption {
      type = types.str;
      default = "nifty.internal";
      description = "Base domain for infrastructure services (e.g. Technitium becomes dns.<domain>).";
    };
  };
}
