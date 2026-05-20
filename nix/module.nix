# NixOS module for nifty-filter (read-only system model)
#
# The system root is read-only. Configuration lives as an HCL file
# on the writable /var partition:
#
#   /var/nifty-filter/nifty-filter.hcl
#
# Systemd services read this file at boot and apply network + firewall config.
# To reconfigure the router: edit the HCL file and reboot.
self:

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nifty-filter;
  inherit (lib) mkEnableOption mkOption types mkIf;

  nifty-filter = self.packages.${pkgs.stdenv.hostPlatform.system}.nifty-filter;

  configDir = "/var/nifty-filter";
  hclFile = "${configDir}/nifty-filter.hcl";

  sodola-switch = self.packages.${pkgs.stdenv.hostPlatform.system}.sodola-switch;
  sodolaConfigDir = "${configDir}/sodola-switch";
  sodola-credentials = "${sodolaConfigDir}/credentials";

  nifty-dashboard = self.packages.${pkgs.stdenv.hostPlatform.system}.nifty-dashboard;

  # Collect enabled optional packages
  optionalPackages = lib.concatLists [
    (lib.optional cfg.packages.sodola-switch.enable sodola-switch)
    (lib.optional cfg.packages.nifty-dashboard.enable nifty-dashboard)
    (lib.optional cfg.packages.iperf.enable pkgs.iperf3)
  ];

  # Shared args passed to each service file
  serviceArgs = { inherit lib pkgs cfg nifty-filter nifty-dashboard sodola-switch configDir hclFile; };

in
{
  options.services.nifty-filter = {
    enable = mkEnableOption "nifty-filter nftables firewall";

    packages = {
      sodola-switch = {
        enable = mkEnableOption "Sodola SL-SWTGW218AS managed switch client";
      };
      nifty-dashboard = {
        enable = mkEnableOption "nifty-dashboard web UI";
      };
      iperf = {
        enable = mkEnableOption "iperf3 bandwidth testing server";
      };
    };
  };

  config = mkIf cfg.enable (lib.mkMerge [
    {
      # Read-only access group for the HCL config file
      users.groups.nifty-config = {};
      users.users.admin.extraGroups = [ "nifty-config" ];

      # Disable NixOS's built-in firewall (we replace it entirely)
      networking.firewall.enable = false;

      # Enable nftables (we manage the ruleset ourselves via the boot service)
      networking.nftables.enable = true;

      # IP forwarding (it's a router)
      # IPv6 forwarding is set per-interface (not all.forwarding) so that
      # the WAN interface can still accept Router Advertisements.
      # all.forwarding=1 forces accept_ra=0 on every interface, breaking DHCPv6-PD.
      boot.kernel.sysctl = {
        "net.ipv4.ip_forward" = 1;
        "net.ipv6.conf.default.forwarding" = 1;
      };

      # Kernel modules for QoS traffic shaping (CAKE qdisc + IFB for download)
      boot.kernelModules = [ "ifb" "sch_cake" ];

      # Make the binary available system-wide
      environment.systemPackages = [ nifty-filter ] ++ optionalPackages;
    }

    (import ./services/init.nix serviceArgs)
    (import ./services/network.nix serviceArgs)
    (import ./services/dnsmasq.nix serviceArgs)
    (import ./services/firewall.nix serviceArgs)
    (import ./services/iperf.nix serviceArgs)
    (import ./services/dashboard.nix serviceArgs)
    (import ./services/sodola-switch.nix serviceArgs)
  ]);
}
