# Immutable NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Router configuration: /var/nifty-filter/router.env
# To reconfigure: edit the env file and reboot.
{ config, pkgs, lib, ... }:

{
  system.stateVersion = "25.05";
  networking.hostName = "nifty-router";

  # --- Immutable root filesystem ---
  fileSystems."/" = {
    device = lib.mkDefault "/dev/vda2";
    fsType = "ext4";
    options = [ "ro" ];
  };
  fileSystems."/boot" = {
    device = lib.mkDefault "/dev/vda1";
    fsType = "vfat";
  };
  fileSystems."/var" = {
    device = lib.mkDefault "/dev/vdb1";
    fsType = "ext4";
    options = [ "rw" "noatime" ];
    neededForBoot = true;
  };

  # Bind-mount mutable paths from /var
  fileSystems."/home" = { device = "/var/home"; options = [ "bind" ]; };
  fileSystems."/root" = { device = "/var/root"; options = [ "bind" ]; };
  fileSystems."/tmp" = { device = "tmpfs"; fsType = "tmpfs"; };

  # Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = false;
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Disable nix operations on the immutable system
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  nix.gc.automatic = false;

  # --- Nifty-filter firewall (reads /var/nifty-filter/router.env at boot) ---
  services.nifty-filter.enable = true;

  # --- Networking ---
  # WAN gets its address via DHCP from upstream.
  # LAN static IP is configured here to match the env file default.
  # If you change SUBNET_LAN in the env file, update this too.
  networking.interfaces.enp1s0.ipv4.addresses = [{
    address = "192.168.10.1";
    prefixLength = 24;
  }];
  networking.interfaces.enp2s0.useDHCP = true;

  # --- DHCP server for LAN clients ---
  services.kea.dhcp4 = {
    enable = true;
    settings = {
      interfaces-config.interfaces = [ "enp1s0" ];
      subnet4 = [{
        id = 1;
        subnet = "192.168.10.0/24";
        pools = [{ pool = "192.168.10.100 - 192.168.10.250"; }];
        option-data = [
          { name = "routers"; data = "192.168.10.1"; }
          { name = "domain-name-servers"; data = "1.1.1.1, 1.0.0.1"; }
        ];
      }];
    };
  };

  # --- DNS resolver ---
  services.resolved = {
    enable = true;
    fallbackDns = [ "1.1.1.1" "1.0.0.1" ];
  };

  # --- SSH ---
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
    };
  };

  # --- User account ---
  users.mutableUsers = false;
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = [
      # Populated from /var/nifty-filter/authorized_keys via bind mount
      # or baked into the image at build time
    ];
  };
  security.sudo.wheelNeedsPassword = false;

  # --- Minimal packages ---
  environment.systemPackages = with pkgs; [
    vim
    htop
    ethtool
    tcpdump
    iproute2
    dig
  ];

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
  sound.enable = false;
}
