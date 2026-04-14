# Example NixOS router configuration using nifty-filter
#
# This shows how to use the nifty-filter NixOS module in a flake-based config.
#
# In your flake.nix:
#
#   inputs.nifty-filter.url = "github:EnigmaCurry/nifty-filter/nixos";
#
#   nixosConfigurations.router = nixpkgs.lib.nixosSystem {
#     system = "x86_64-linux";
#     modules = [
#       nifty-filter.nixosModules.default
#       ./configuration.nix  # this file
#     ];
#   };
{ config, pkgs, ... }:

{
  networking.hostName = "router";

  services.nifty-filter = {
    enable = true;

    interfaces = {
      lan = "enp1s0";
      wan = "enp2s0";
    };

    subnet.lan = "192.168.10.1/24";

    icmp.acceptLan = [
      "echo-request"
      "echo-reply"
      "destination-unreachable"
      "time-exceeded"
    ];

    tcp = {
      acceptLan = [ 22 80 443 ];
      # Forward port 8080 on LAN to an internal web server
      forwardLan = [
        { incomingPort = 8080; destinationIp = "192.168.10.50"; destinationPort = 80; }
      ];
      # Expose an internal service to the WAN
      forwardWan = [
        { incomingPort = 443; destinationIp = "192.168.10.50"; destinationPort = 443; }
      ];
    };

    udp = {
      # Forward DNS to an internal resolver
      forwardLan = [
        { incomingPort = 53; destinationIp = "192.168.10.2"; destinationPort = 53; }
      ];
    };
  };
}
