# NixOS module additions for PVE disk images
#
# When included in the system config, this module:
# - Marks the system as a PVE install (skips disk ops in nifty-install)
# - Seeds SSH authorized keys on first boot from build-time parameter
# - Provides minimal bootstrap networking via fw_cfg mgmt MAC
# - Shows a first-boot banner prompting nifty-install
{ config, pkgs, lib, sshKeys ? "", gitBranch ? "master", nifty-filter-pkg, ... }:

{
  # Mark this as a PVE install (nifty-install checks this to skip disk ops)
  environment.etc."nifty-filter/pve-install".text = "pve";

  # Record build branch for nifty-upgrade
  environment.etc."nifty-filter/build-branch".text = gitBranch;

  # nifty-install available in PATH (runs config wizard only on PVE)
  environment.systemPackages = [
    nifty-filter-pkg
    (pkgs.writeShellScriptBin "nifty-install" ''exec nifty-filter install "$@"'')
  ];

  # Seed SSH authorized keys on first boot from build-time parameter.
  # Keys are baked into the NixOS config, written to /var on first boot.
  systemd.services.nifty-pve-ssh-keys = {
    description = "Seed SSH authorized keys (PVE image)";
    wantedBy = [ "multi-user.target" ];
    before = [ "sshd.service" ];
    unitConfig.ConditionPathExists = "!/var/home/admin/.ssh/authorized_keys";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      mkdir -p /var/home/admin/.ssh
      chmod 700 /var/home/admin/.ssh
      cat > /var/home/admin/.ssh/authorized_keys <<'KEYS'
      ${sshKeys}
      KEYS
      # Strip leading whitespace from heredoc
      ${pkgs.gnused}/bin/sed -i 's/^      //' /var/home/admin/.ssh/authorized_keys
      chmod 600 /var/home/admin/.ssh/authorized_keys
      chown -R 1000:100 /var/home/admin
    '';
  };

  # Bootstrap networking: configure mgmt interface from fw_cfg before
  # nifty-install has run. This ensures SSH access on first boot.
  # After nifty-install writes the env file, nifty-network takes over.
  systemd.services.nifty-pve-network = {
    description = "Bootstrap mgmt network from fw_cfg (PVE)";
    wantedBy = [ "multi-user.target" ];
    before = [ "sshd.service" "nifty-network.service" ];
    after = [ "systemd-udevd-settle.service" "nifty-filter-init.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.gnugrep ];
    script = ''
      MGMT_MAC=""
      if [ -f /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw ]; then
        MGMT_MAC=$(cat /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
      fi
      if [ -z "$MGMT_MAC" ]; then
        echo "No mgmt MAC in fw_cfg, skipping bootstrap network"
        exit 0
      fi
      echo "Looking for mgmt MAC: '$MGMT_MAC'"

      # Bring up all interfaces first
      for iface in $(ip -o link show | grep -oP '\d+: \K[^:]+' | grep -v '^lo$'); do
        ip link set "$iface" up || true
      done
      sleep 1

      # Find interface with mgmt MAC and assign mgmt IP
      FOUND=""
      for iface in $(ip -o link show | grep -oP '\d+: \K[^:]+' | grep -v '^lo$'); do
        MAC=$(ip -o link show "$iface" 2>/dev/null | grep -oP 'link/ether \K[^ ]+' | tr '[:upper:]' '[:lower:]')
        echo "  $iface: '$MAC'"
        if [ "$MAC" = "$MGMT_MAC" ]; then
          ip addr add 10.99.0.1/24 dev "$iface"
          echo "Bootstrap: $iface ($MAC) -> 10.99.0.1/24"
          FOUND=1
          break
        fi
      done
      if [ -z "$FOUND" ]; then
        echo "WARNING: No interface found matching mgmt MAC $MGMT_MAC"
      fi
    '';
  };

  # No root password — console auto-logs in as admin, SSH is key-only
  users.users.root.hashedPassword = lib.mkForce "!";

  # No password auth over SSH — keys are pre-installed
  services.openssh.settings.PasswordAuthentication = lib.mkForce false;

  # PVE first-boot banner
  environment.interactiveShellInit = lib.mkForce ''
    export PS1='\[\e[1;33m\][PVE]\[\e[0m\] \u@\h:\w\$ '
    if grep -q 'nifty.maintenance=1' /proc/cmdline 2>/dev/null; then
      export PS1='\[\e[1;31m\][MAINTENANCE]\[\e[0m\] \u@\h:\w\$ '
      echo ""
      echo -e "\e[1;31m  *** MAINTENANCE MODE — root filesystem is READ-WRITE ***\e[0m"
      echo ""
      echo "  Upgrade system:  nifty-upgrade"
      echo "  Return to normal: systemctl reboot"
      echo ""
    elif ! [ -f /var/nifty-filter/nifty-filter.hcl ]; then
      echo ""
      echo -e "\e[1;33m  nifty-filter PVE image — not yet configured\e[0m"
      echo ""
      echo "  Run the configuration wizard:"
      echo "    nifty-install"
      echo ""
    else
      echo ""
      echo "  Configure:  nifty-config"
      echo "  Upgrade:    nifty-upgrade"
      echo ""
    fi
  '';
}
