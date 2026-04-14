# ISO image configuration
# Extends the base system with live ISO support.
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/iso-image.nix"
    "${modulesPath}/profiles/all-hardware.nix"
  ];

  # ISO settings
  isoImage = {
    isoName = "nifty-router-${config.system.nixos.label}.iso";
    volumeID = "NIFTY_ROUTER";
    makeEfiBootable = true;
    makeBiosBootable = true;
  };

  # Override the systemd-boot config from system.nix for the live ISO
  # (the ISO uses its own bootloader)
  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = lib.mkForce false;

  # Allow login on the console for initial setup
  users.users.admin.initialPassword = "nifty";

  # Show setup instructions on login
  environment.etc."motd".text = ''

    ┌──────────────────────────────────────────────┐
    │          nifty-router live ISO                │
    │                                              │
    │  Login: admin / nifty                        │
    │                                              │
    │  1. Identify interfaces:  ip link            │
    │  2. Edit config:                             │
    │     sudo vim /etc/nixos/configuration.nix    │
    │  3. Rebuild:                                 │
    │     sudo nixos-rebuild switch                │
    │  4. Install to disk:                         │
    │     sudo nixos-install                       │
    │                                              │
    │  Config: /etc/nixos/configuration.nix        │
    └──────────────────────────────────────────────┘

  '';

  # Copy the editable config into /etc/nixos on the live system
  environment.etc."nixos/configuration.nix" = {
    source = ./system.nix;
    mode = "0644";
  };
}
