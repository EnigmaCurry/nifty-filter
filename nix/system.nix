# Minimal NixOS router system configuration
# This is the base system that gets built into an ISO.
# Users customize it by editing the nifty-filter options below.
{ config, pkgs, lib, ... }:

{
  # --- System basics ---
  system.stateVersion = "25.05";
  networking.hostName = "nifty-router";

  # Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  # Minimal kernel with router-relevant modules
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # --- Nifty-filter firewall ---
  # Edit these to match your hardware and network.
  # After changing, rebuild with: nixos-rebuild switch
  services.nifty-filter = {
    enable = true;

    interfaces = {
      lan = "enp1s0";   # change to your LAN interface
      wan = "enp2s0";   # change to your WAN interface
    };

    subnet.lan = "192.168.10.1/24";

    tcp.acceptLan = [ 22 ];  # SSH only by default
  };

  # --- Networking ---
  # Static IP on the LAN side (this is the gateway address)
  networking.interfaces.enp1s0.ipv4.addresses = [{
    address = "192.168.10.1";
    prefixLength = 24;
  }];

  # WAN gets its address via DHCP from upstream
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

  # --- DNS resolver (forwarding only) ---
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
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    # Replace with your SSH public key:
    openssh.authorizedKeys.keys = [
      # "ssh-ed25519 AAAA..."
    ];
  };
  security.sudo.wheelNeedsPassword = false;

  # --- Minimal packages ---
  environment.systemPackages = with pkgs; [
    vim
    htop
    ethtool
    tcpdump
    nftables
    iproute2
    dig
  ];

  # Keep the system lean
  documentation.enable = false;
  services.xserver.enable = false;
  sound.enable = false;
}
