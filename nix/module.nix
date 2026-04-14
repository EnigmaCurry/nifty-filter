# NixOS module for nifty-filter
# Usage: in your NixOS configuration flake, add nifty-filter as an input
# and include this module:
#
#   nixosModules = [ nifty-filter.nixosModules.default ];
#
#   services.nifty-filter = {
#     enable = true;
#     interfaces.lan = "enp1s0";
#     interfaces.wan = "enp2s0";
#     subnet.lan = "192.168.10.1/24";
#   };
self:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-filter;
  inherit (lib) mkEnableOption mkOption types mkIf concatStringsSep;

  # Build the env file content from NixOS options
  mkEnvFile = ''
    INTERFACE_LAN=${cfg.interfaces.lan}
    INTERFACE_WAN=${cfg.interfaces.wan}
    SUBNET_LAN=${cfg.subnet.lan}
    ICMP_ACCEPT_LAN=${concatStringsSep "," cfg.icmp.acceptLan}
    ICMP_ACCEPT_WAN=${concatStringsSep "," cfg.icmp.acceptWan}
    TCP_ACCEPT_LAN=${concatStringsSep "," (map toString cfg.tcp.acceptLan)}
    UDP_ACCEPT_LAN=${concatStringsSep "," (map toString cfg.udp.acceptLan)}
    TCP_ACCEPT_WAN=${concatStringsSep "," (map toString cfg.tcp.acceptWan)}
    UDP_ACCEPT_WAN=${concatStringsSep "," (map toString cfg.udp.acceptWan)}
    TCP_FORWARD_LAN=${concatStringsSep "," (map formatForwardRoute cfg.tcp.forwardLan)}
    UDP_FORWARD_LAN=${concatStringsSep "," (map formatForwardRoute cfg.udp.forwardLan)}
    TCP_FORWARD_WAN=${concatStringsSep "," (map formatForwardRoute cfg.tcp.forwardWan)}
    UDP_FORWARD_WAN=${concatStringsSep "," (map formatForwardRoute cfg.udp.forwardWan)}
  '';

  formatForwardRoute = r: "${toString r.incomingPort}:${r.destinationIp}:${toString r.destinationPort}";

  nifty-filter = self.packages.${pkgs.system}.nifty-filter;

  # Generate the nftables ruleset at build time
  generatedRuleset = pkgs.runCommand "nifty-filter-ruleset" {
    envFile = pkgs.writeText "nifty-filter.env" mkEnvFile;
  } ''
    ${nifty-filter}/bin/nifty-filter nftables --env-file $envFile --strict-env > $out
  '';

  forwardRouteType = types.submodule {
    options = {
      incomingPort = mkOption {
        type = types.port;
        description = "Port to listen on for incoming traffic.";
      };
      destinationIp = mkOption {
        type = types.str;
        description = "IP address to forward traffic to.";
      };
      destinationPort = mkOption {
        type = types.port;
        description = "Port on the destination host.";
      };
    };
  };

in
{
  options.services.nifty-filter = {
    enable = mkEnableOption "nifty-filter nftables firewall";

    interfaces = {
      lan = mkOption {
        type = types.str;
        description = "LAN network interface name.";
        example = "enp1s0";
      };
      wan = mkOption {
        type = types.str;
        description = "WAN network interface name.";
        example = "enp2s0";
      };
    };

    subnet = {
      lan = mkOption {
        type = types.str;
        description = "LAN subnet in CIDR notation.";
        example = "192.168.10.1/24";
      };
    };

    icmp = {
      acceptLan = mkOption {
        type = types.listOf types.str;
        default = [ "echo-request" "echo-reply" "destination-unreachable" "time-exceeded" ];
        description = "ICMP types to accept on the LAN interface.";
      };
      acceptWan = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "ICMP types to accept on the WAN interface.";
      };
    };

    tcp = {
      acceptLan = mkOption {
        type = types.listOf types.port;
        default = [ 22 80 443 ];
        description = "TCP ports to accept on the LAN interface.";
      };
      acceptWan = mkOption {
        type = types.listOf types.port;
        default = [ ];
        description = "TCP ports to accept on the WAN interface.";
      };
      forwardLan = mkOption {
        type = types.listOf forwardRouteType;
        default = [ ];
        description = "TCP port forwarding rules for LAN clients.";
        example = [{ incomingPort = 8080; destinationIp = "192.168.1.100"; destinationPort = 80; }];
      };
      forwardWan = mkOption {
        type = types.listOf forwardRouteType;
        default = [ ];
        description = "TCP port forwarding rules for WAN peers.";
        example = [{ incomingPort = 1234; destinationIp = "192.168.1.1"; destinationPort = 1234; }];
      };
    };

    udp = {
      acceptLan = mkOption {
        type = types.listOf types.port;
        default = [ ];
        description = "UDP ports to accept on the LAN interface.";
      };
      acceptWan = mkOption {
        type = types.listOf types.port;
        default = [ ];
        description = "UDP ports to accept on the WAN interface.";
      };
      forwardLan = mkOption {
        type = types.listOf forwardRouteType;
        default = [ ];
        description = "UDP port forwarding rules for LAN clients.";
      };
      forwardWan = mkOption {
        type = types.listOf forwardRouteType;
        default = [ ];
        description = "UDP port forwarding rules for WAN peers.";
      };
    };
  };

  config = mkIf cfg.enable {
    # Disable NixOS's built-in firewall (we replace it entirely)
    networking.firewall.enable = false;

    # Enable nftables and inject the generated ruleset
    networking.nftables = {
      enable = true;
      ruleset = builtins.readFile generatedRuleset;
    };

    # Ensure IP forwarding is enabled (it's a router)
    boot.kernel.sysctl = {
      "net.ipv4.ip_forward" = 1;
    };
  };
}
