# ISO image configuration for nifty-router
#
# The ISO boots into an immutable system. The live environment
# uses tmpfs for /var so edits to router.env persist until reboot.
# Install to disk for persistent configuration.
#
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/iso-image.nix"
    "${modulesPath}/profiles/all-hardware.nix"
  ];

  image.baseName = lib.mkForce "nifty-router-${config.system.nixos.label}";
  isoImage = {
    volumeID = "NIFTY_ROUTER";
    makeEfiBootable = true;
    makeBiosBootable = true;
  };

  # Override immutable filesystem mounts from system.nix
  # The ISO module provides its own squashfs root and tmpfs overlay,
  # so we disable the disk-based mounts and use tmpfs for /var.
  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = lib.mkForce false;

  # Allow console login for initial setup
  users.users.admin.initialPassword = lib.mkForce "nifty";
  services.openssh.settings.PasswordAuthentication = lib.mkForce true;

  environment.etc."motd".text = ''

    ================================
     nifty-router live environment
    ================================

     Login: admin / nifty

     Router config: /var/nifty-filter/router.env

     1. Identify interfaces:   ip link
     2. Edit config:           sudo vim /var/nifty-filter/router.env
     3. Apply without reboot:  sudo systemctl restart nifty-filter
     4. Install to disk:       sudo nifty-install /dev/sdX

     Note: on the live ISO, /var is tmpfs.
     Changes are lost on reboot until installed to disk.

    ================================

  '';
}
