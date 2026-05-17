# iperf3 bandwidth testing server.
#
# Runs with DynamicUser (no persistent user, minimal privileges).
# Listens on the port configured in HCL (default: 5201).
# Optional — only started when packages.iperf.enable is true.

{ lib, pkgs, cfg, nifty-filter, hclFile, ... }:

let
  inherit (lib) mkIf;
in
{
  systemd.services.nifty-iperf = mkIf cfg.packages.iperf.enable {
    description = "iperf3 bandwidth testing server";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "nifty-filter.service" ];

    serviceConfig = {
      Type = "simple";
      ExecStart = "${pkgs.bash}/bin/bash -c 'PORT=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} iperf-port); exec ${pkgs.iperf3}/bin/iperf3 --server --port $PORT'";
      Restart = "on-failure";
      RestartSec = "5s";
      DynamicUser = true;
    };
  };
}
