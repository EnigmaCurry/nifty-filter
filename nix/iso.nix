# ISO image configuration for nifty-filter
#
# The ISO boots into an immutable system. The live environment
# uses tmpfs for /var so edits to router.env persist until reboot.
# Install to disk for persistent configuration.
#
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, version ? "unknown", installedToplevel, scriptWizard, ... }:

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

  # Disable nifty-filter service on the live ISO (no router config yet)
  services.nifty-filter.enable = lib.mkForce false;

  # Install script and tools available in PATH
  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "nifty-install" (builtins.readFile ./nifty-install.sh))
    scriptWizard
    parted
    dosfstools
    e2fsprogs
    git
  ];

  # Ship the default env file where the installer can find it
  environment.etc."nifty-filter/default-router.env".source = ./default-router.env;

  # Make the installed system closure available to the installer.
  # This is the disk-based system (with filesystem.nix), not the live ISO system.
  environment.etc."nifty-filter/installed-system".text = "${installedToplevel}";

  # Include the installed system closure in the ISO's nix store
  isoImage.storeContents = [ installedToplevel ];

  # Allow console login for initial setup
  users.users.admin.initialPassword = lib.mkForce "nifty";
  services.openssh.settings.PasswordAuthentication = lib.mkForce true;

  # Pre-login banner on console — generated at boot with all interface IPs
  systemd.services.update-issue = {
    description = lib.mkForce "Generate /etc/issue with interface IPs";
    script = lib.mkForce ''
      {
        echo ""
        echo -e "  \e[1mnifty-filter\e[0m live ISO"
        echo ""
        ip -4 -o addr show scope global | awk '{printf "  %-12s %s\n", $2, $4}'
        echo ""
        echo "  Login:  admin / nifty"
        echo ""
      } > /etc/issue
    '';
  };

  # Post-login instructions
  users.motd = ''

    Setup:

     0. If network is not up, configure it:
          sudo nmtui

     1. Add your SSH public key (from your workstation):
          ssh-copy-id admin@<this-host>

     2. Reconnect with key auth:
          ssh admin@<this-host>

     3. Install (interactive wizard selects disk, interfaces, DHCP):
          sudo nifty-install

     4. Reboot into the installed system

    The installer will refuse to run under password auth.
    Your SSH key and host fingerprint are preserved in the install.

  '';
}
