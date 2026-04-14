# Immutable NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Router configuration: /var/nifty-filter/router.env
# To reconfigure: edit the env file and reboot.
{ config, pkgs, lib, ... }:

{
  system.stateVersion = "25.05";
  networking.hostName = "nifty-filter";

  # Boot (filesystem mounts are in filesystem.nix, not here,
  # so the ISO can provide its own without conflicts)
  boot.loader.systemd-boot.enable = lib.mkDefault true;
  boot.loader.efi.canTouchEfiVariables = lib.mkDefault false;
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
    settings.Resolve.FallbackDNS = [ "1.1.1.1" "1.0.0.1" ];
  };

  # --- SSH ---
  services.openssh = {
    enable = true;
    # Persist host keys on /var so they survive image upgrades
    hostKeys = [
      { path = "/var/nifty-filter/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
      { path = "/var/nifty-filter/ssh/ssh_host_rsa_key"; type = "rsa"; bits = 4096; }
    ];
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      X11Forwarding = false;
      MaxAuthTries = 3;
      ClientAliveInterval = 300;
      ClientAliveCountMax = 2;
    };
  };

  # --- User account ---
  users.mutableUsers = false;
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    # SSH authorized keys live in ~/.ssh/authorized_keys (standard path).
    # Since /home is bind-mounted from /var/home, this persists across reboots.
    # Use ssh-copy-id to add keys.
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
}
