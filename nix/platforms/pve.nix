# NixOS module additions for PVE disk images
#
# When included in the system config, this module:
# - Marks the system as a PVE install
# - Seeds SSH authorized keys on first boot from build-time parameter
# - Provides minimal bootstrap networking via fw_cfg mgmt MAC
{ config, pkgs, lib, sshKeys ? "", gitBranch ? "master", nifty-filter-pkg, ... }:

{
  # Mark this as a PVE install
  environment.etc."nifty-filter/pve-install".text = "pve";

  # Record build branch for nifty-upgrade
  environment.etc."nifty-filter/build-branch".text = gitBranch;

  environment.systemPackages = [
    nifty-filter-pkg
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

  # Auto-populate the default HCL with real MAC addresses from fw_cfg.
  # Runs once after nifty-filter-init seeds the default config, patching
  # placeholder MACs with real ones discovered from fw_cfg + PCI bus order.
  systemd.services.nifty-pve-init-config = {
    description = "Populate HCL config with real MACs from fw_cfg";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-filter-init.service" "systemd-udevd-settle.service" ];
    before = [ "nifty-link.service" "nifty-network.service" "nifty-filter.service" ];
    unitConfig.ConditionPathExists = "!/var/nifty-filter/.pve-init-done";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.gnugrep pkgs.gnused pkgs.coreutils pkgs.gawk ];
    script = ''
      HCL="/var/nifty-filter/nifty-filter.hcl"
      if [ ! -f "$HCL" ]; then
        echo "No HCL file found, skipping"
        exit 0
      fi

      # Read fw_cfg
      FWCFG="/sys/firmware/qemu_fw_cfg/by_name/opt/nifty"
      if [ ! -d "$FWCFG" ]; then
        echo "No fw_cfg data, skipping"
        touch /var/nifty-filter/.pve-init-done
        exit 0
      fi

      MGMT_MAC=""
      NIC_ROLES=""
      [ -f "$FWCFG/mgmt_mac/raw" ] && MGMT_MAC=$(cat "$FWCFG/mgmt_mac/raw" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
      [ -f "$FWCFG/nic_roles/raw" ] && NIC_ROLES=$(cat "$FWCFG/nic_roles/raw" | tr -d '[:space:]')

      if [ -z "$NIC_ROLES" ]; then
        echo "No nic_roles in fw_cfg, skipping"
        touch /var/nifty-filter/.pve-init-done
        exit 0
      fi

      echo "fw_cfg: mgmt_mac=$MGMT_MAC nic_roles=$NIC_ROLES"

      # Split roles (colon-separated)
      IFS=':' read -ra ROLES <<< "$NIC_ROLES"

      # Build MAC map: first check fw_cfg for per-role MACs (virtual NICs),
      # then discover PCI NICs by bus address for any missing
      declare -A ROLE_MAC
      NEED_PCI=()
      for role in "''${ROLES[@]}"; do
        if [ -f "$FWCFG/''${role}_mac/raw" ]; then
          MAC=$(cat "$FWCFG/''${role}_mac/raw" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
          ROLE_MAC[$role]="$MAC"
          echo "  $role: $MAC (from fw_cfg)"
        else
          NEED_PCI+=("$role")
        fi
      done

      # Discover PCI NICs (sorted by bus address) for roles without fw_cfg MACs
      if [ ''${#NEED_PCI[@]} -gt 0 ]; then
        PCI_NICS=()
        for iface in /sys/class/net/*/device; do
          [ -e "$iface" ] || continue
          IFNAME=$(basename $(dirname "$iface"))
          [ "$IFNAME" = "lo" ] && continue
          MAC=$(ip -o link show "$IFNAME" 2>/dev/null | grep -oP 'link/ether \K[^ ]+' | tr '[:upper:]' '[:lower:]')
          # Skip mgmt interface
          [ "$MAC" = "$MGMT_MAC" ] && continue
          # Skip any interface already assigned from fw_cfg
          SKIP=""
          for assigned_mac in "''${ROLE_MAC[@]}"; do
            [ "$MAC" = "$assigned_mac" ] && SKIP=1 && break
          done
          [ -n "$SKIP" ] && continue
          BUS=$(readlink -f "$iface" | grep -oP '/[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-9a-f]/' | tail -1 | tr -d '/')
          # Use pipe delimiter to avoid MAC colon conflicts
          PCI_NICS+=("$BUS|$IFNAME|$MAC")
        done
        # Sort by PCI bus address
        IFS=$'\n' SORTED=($(sort <<< "''${PCI_NICS[*]}")); unset IFS

        for i in "''${!NEED_PCI[@]}"; do
          if [ "$i" -lt "''${#SORTED[@]}" ]; then
            role="''${NEED_PCI[$i]}"
            MAC=$(echo "''${SORTED[$i]}" | cut -d'|' -f3)
            IFNAME=$(echo "''${SORTED[$i]}" | cut -d'|' -f2)
            ROLE_MAC[$role]="$MAC"
            echo "  $role: $MAC ($IFNAME, PCI)"
          fi
        done
      fi

      # Build new interfaces block
      IFACES_FILE=$(mktemp)
      echo "interfaces {" > "$IFACES_FILE"
      for role in "''${ROLES[@]}"; do
        MAC="''${ROLE_MAC[$role]:-}"
        if [ -n "$MAC" ]; then
          echo "  $role {" >> "$IFACES_FILE"
          echo "    mac  = \"$MAC\"" >> "$IFACES_FILE"
          echo "    name = \"$role\"" >> "$IFACES_FILE"
          echo "  }" >> "$IFACES_FILE"
        fi
      done
      if [ -n "$MGMT_MAC" ]; then
        echo "  mgmt {" >> "$IFACES_FILE"
        echo "    mac    = \"$MGMT_MAC\"" >> "$IFACES_FILE"
        echo "    name   = \"mgmt\"" >> "$IFACES_FILE"
        echo "    subnet = \"10.99.0.1/24\"" >> "$IFACES_FILE"
        echo "  }" >> "$IFACES_FILE"
      fi
      echo "}" >> "$IFACES_FILE"


      echo "New interfaces block:"
      cat "$IFACES_FILE"

      # Replace the interfaces block in HCL using awk
      # Matches from "interfaces {" through the balanced closing "}"
      awk -v replacement="$(cat "$IFACES_FILE")" '
        /^(# Interfaces:|# To give|# and specify|# name you|# Find your)/ && !in_block { skipping_comments=1; next }
        /^interfaces\s*\{/ {
          in_block=1; depth=1
          printf "%s\n", replacement
          next
        }
        in_block {
          if ($0 ~ /\{/) depth++
          if ($0 ~ /\}/) depth--
          if (depth == 0) { in_block=0 }
          next
        }
        skipping_comments && /^[^#]/ { skipping_comments=0 }
        skipping_comments { next }
        { print }
      ' "$HCL" > "$HCL.tmp"
      mv "$HCL.tmp" "$HCL"
      chmod 0664 "$HCL"
      chown root:wheel "$HCL"
      rm -f "$IFACES_FILE"

      # Handle infra NIC (virtual bridge for Step-CA / services communication)
      INFRA_MAC=""
      [ -f "$FWCFG/infra_mac/raw" ] && INFRA_MAC=$(cat "$FWCFG/infra_mac/raw" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
      if [ -n "$INFRA_MAC" ]; then
        echo "  infra MAC: $INFRA_MAC (from fw_cfg)"
        sed -i "s/mac  = \"aa:bb:cc:dd:ee:10\"/mac  = \"$INFRA_MAC\"/" "$HCL"
      fi

      echo "HCL updated with real MACs"
      touch /var/nifty-filter/.pve-init-done
    '';
  };

  # Bootstrap networking: configure mgmt interface from fw_cfg.
  # This ensures SSH access on first boot before nifty-network takes over.
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

  # On PVE, nifty-link must wait for the HCL to be populated with real MACs
  systemd.services.nifty-link.after = [ "nifty-pve-init-config.service" "systemd-udevd-settle.service" ];

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
    else
      echo ""
      echo "  Config:   /var/nifty-filter/nifty-filter.hcl"
      echo "  Upgrade:  nifty-upgrade"
      echo ""
    fi
  '';
}
