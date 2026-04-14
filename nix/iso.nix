# ISO image configuration for nifty-filter
#
# The ISO boots into an immutable system. The live environment
# uses tmpfs for /var so edits to router.env persist until reboot.
# Install to disk for persistent configuration.
#
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, version ? "unknown", ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/iso-image.nix"
    "${modulesPath}/profiles/all-hardware.nix"
  ];

  image.baseName = lib.mkForce "nifty-filter-${version}";
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

  # NetworkManager for live ISO network setup (nmtui)
  networking.networkmanager.enable = true;

  # Install script and tools available in PATH
  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "nifty-install" (builtins.readFile ./nifty-install.sh))
    parted
    dosfstools
    e2fsprogs
    git
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
  users.motd = ''

    Setup:

     0. If network is not up, configure it:
          sudo nmtui

     1. Add your SSH public key (from your workstation):
          ssh-copy-id admin@<this-host>

     2. Reconnect with key auth:
          ssh admin@<this-host>

     3. Install to disk:
          sudo nifty-install /dev/sdX

     4. Reboot into the installed system

    The installer will refuse to run under password auth.
    Your SSH key and host fingerprint are preserved in the install.

  '';
}
