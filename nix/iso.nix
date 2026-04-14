# ISO image configuration for nifty-filter
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

  image.baseName = lib.mkForce "nifty-filter-${config.system.nixos.label}";
  isoImage = {
    volumeID = "NIFTY_FILTER";
    makeEfiBootable = true;
    makeBiosBootable = true;
  };

  # Override immutable filesystem mounts from system.nix
  # The ISO module provides its own squashfs root and tmpfs overlay,
  # so we disable the disk-based mounts and use tmpfs for /var.
  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = lib.mkForce false;

  # Install script available in PATH
  environment.systemPackages = [
    (pkgs.writeShellScriptBin "nifty-install" (builtins.readFile ./nifty-install.sh))
  ];

  # Ship the default env file where the installer can find it
  environment.etc."nifty-filter/default-router.env".source = ./default-router.env;

  # Allow console login for initial setup
  users.users.admin.initialPassword = lib.mkForce "nifty";
  services.openssh.settings.PasswordAuthentication = lib.mkForce true;

  # Pre-login banner on console
  environment.etc."issue".text = ''

    \e[1mnifty-filter\e[0m live ISO (\n) \l
    IP: \4

    Login:  admin / nifty

  '';

  # Post-login instructions
  environment.etc."motd".text = ''

    Router config: /var/nifty-filter/router.env

     1. Identify interfaces:   ip link
     2. Install to disk:       sudo nifty-install /dev/sdX
     3. Edit config:           sudo vim /var/nifty-filter/router.env
     4. Apply without reboot:  sudo systemctl restart nifty-filter

    Note: on the live ISO, /var is tmpfs.
    Changes are lost on reboot until installed to disk.

  '';
}
