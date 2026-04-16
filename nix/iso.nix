# ISO image configuration for nifty-filter
#
# The ISO boots into a read-only system. The live environment
# uses tmpfs for /var so edits to nifty-filter.env persist until reboot.
# Install to disk for persistent configuration.
#
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, version ? "unknown", installedToplevel, gitBranch ? "master", nifty-filter-pkg, ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/iso-image.nix"
  ];

  # Minimal hardware support — skip the full linux-firmware bundle from all-hardware.nix
  hardware.enableRedistributableFirmware = lib.mkForce false;

  image.baseName = lib.mkForce "nifty-filter-${version}";
  isoImage = {
    volumeID = "NIFTY_FILTER";
    makeEfiBootable = true;
    makeBiosBootable = true;
    squashfsCompression = "zstd -Xcompression-level 19";
  };

  # Override read-only filesystem mounts from system.nix
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
    nifty-filter-pkg
    (writeShellScriptBin "nifty-install" ''exec nifty-filter install "$@"'')
    parted
    dosfstools
    e2fsprogs
  ];

  # Ship the default env file where the installer can find it
  environment.etc."nifty-filter/default-nifty-filter.env".source = ./default-nifty-filter.env;

  # Make the installed system closure available to the installer.
  # This is the disk-based system (with filesystem.nix), not the live ISO system.
  environment.etc."nifty-filter/installed-system".text = "${installedToplevel}";

  # Record which branch this ISO was built from so the installer
  # can write it to /var/nifty-filter/branch for nifty-upgrade.
  environment.etc."nifty-filter/build-branch".text = "${gitBranch}";

  # Include the installed system closure in the ISO's nix store
  isoImage.storeContents = [ installedToplevel ];

  # Allow console login for initial setup
  users.users.admin.initialPassword = lib.mkForce "nifty";
  services.openssh.settings.PasswordAuthentication = lib.mkForce true;

  # Use /etc/issue directly (writable on the live ISO)
  services.getty.extraArgs = lib.mkForce [ ];
  environment.etc."issue".text = lib.mkForce ''

    \e[1mnifty-filter\e[0m live installer (\n) \l
    \4

    Login:  admin / nifty

    Connect via SSH to install. Use this console
    only if you need to configure networking (nmtui).

  '';

  users.motd = "";

  environment.interactiveShellInit = lib.mkForce ''
    export PS1='\[\e[1;32m\][LIVE ISO]\[\e[0m\] \u@\h:\w\$ '
    if [ -s "$HOME/.ssh/authorized_keys" ]; then
      echo ""
      echo "  SSH key installed. Ready to install."
      echo ""
      echo "   1. Run :"
      echo "        nifty-install"
      echo ""
    else
      echo ""
      echo "  Setup:"
      echo ""
      echo "   0. If network is not up, configure it:"
      echo "        sudo nmtui"
      echo ""
      echo "   1. From your workstation, add your SSH public key:"
      echo "        ssh-copy-id admin@<this-host>"
      echo ""
      echo "   2. Connect from your workstation (using your key):"
      echo "        ssh admin@<this-host>"
      echo ""
      echo "  Once your key is installed, additional instructions will be given"
      echo "  when you connect."
      echo
    fi
  '';
}
