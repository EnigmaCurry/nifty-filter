# Firewall and QoS services.
#
#   - nifty-filter: generates nftables ruleset from HCL config and applies it.
#     Falls back to a lockdown ruleset if no config exists.
#   - nifty-qos: applies CAKE traffic shaping on the WAN interface with
#     per-VLAN HTB classes and IFB-based download shaping.
#
# Both are oneshot services that run as root (nft and tc require it).

{ pkgs, nifty-filter, hclFile, ... }:

{
  # Apply nftables rules from the HCL config at every boot
  systemd.services.nifty-filter = {
    description = "Apply nifty-filter nftables ruleset";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-pre.target" "nifty-filter-init.service" "nifty-network.service" ];
    before = [ "network.target" ];
    wants = [ "network-pre.target" ];

    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${pkgs.bash}/bin/bash -c '${nifty-filter}/bin/nifty-filter nftables --config ${hclFile} | ${pkgs.nftables}/bin/nft -f -'";
      ExecStop = "${pkgs.nftables}/bin/nft flush ruleset";
    };

    preStart = ''
      if [ ! -f ${hclFile} ]; then
        echo "ERROR: ${hclFile} not found. Applying emergency lockdown rules."
        ${pkgs.nftables}/bin/nft -f - <<'LOCKDOWN'
      flush ruleset
      table inet filter {
        chain input {
          type filter hook input priority 0; policy drop;
          ct state established,related accept
          iif "lo" accept
        }
        chain forward {
          type filter hook forward priority 0; policy drop;
        }
        chain output {
          type filter hook output priority 0; policy drop;
          oif "lo" accept
        }
      }
      LOCKDOWN
        exit 1
      fi
    '';
  };

  # QoS traffic shaping (CAKE qdisc) — runs only when qos block is configured
  systemd.services.nifty-qos = {
    description = "Apply QoS traffic shaping (CAKE)";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "nifty-filter.service" ];

    path = [ pkgs.iproute2 pkgs.kmod pkgs.bash ];

    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${pkgs.bash}/bin/bash -o pipefail -c '${nifty-filter}/bin/nifty-filter qos --config ${hclFile} | ${pkgs.bash}/bin/bash'";
      ExecStop = "${pkgs.bash}/bin/bash -c 'WAN_IFACE=$(${nifty-filter}/bin/nifty-filter get -c ${hclFile} wan-name 2>/dev/null); ${pkgs.iproute2}/bin/tc qdisc del dev \"$WAN_IFACE\" root 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev \"$WAN_IFACE\" ingress 2>/dev/null; ${pkgs.iproute2}/bin/tc qdisc del dev ifb0 root 2>/dev/null; ${pkgs.iproute2}/bin/ip link set ifb0 down 2>/dev/null; true'";
    };
  };
}
