# ISO image configuration for nifty-filter
#
# The ISO boots into a read-only system. The live environment
# uses tmpfs for /var so edits to nifty-filter.env persist until reboot.
# Install to disk for persistent configuration.
#
# Build with: nix build .#iso
{ config, pkgs, lib, modulesPath, version ? "unknown", installedToplevel, gitBranch ? "master", nifty-filter-pkg, ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/iso-image.nix"
  ];

  # Minimal hardware support — skip the full linux-firmware bundle from all-hardware.nix
  hardware.enableRedistributableFirmware = lib.mkForce false;

  image.baseName = lib.mkForce "nifty-filter-${version}";
  isoImage = {
    volumeID = "NIFTY_FILTER";
    makeEfiBootable = true;
    makeBiosBootable = true;
    squashfsCompression = "zstd -Xcompression-level 19";
  };

  # Override read-only filesystem mounts from system.nix
  # The ISO module provides its own squashfs root and tmpfs overlay,
  # so we disable the disk-based mounts and use tmpfs for /var.
  boot.loader.systemd-boot.enable = lib.mkForce false;
  boot.loader.efi.canTouchEfiVariables = lib.mkForce false;

  # Disable NetworkManager — the live ISO runs its own DHCP server
  networking.networkmanager.enable = lib.mkForce false;

  # Disable nifty-filter service on the live ISO (no router config yet)
  services.nifty-filter.enable = lib.mkForce false;

  # Open DHCP server port on the NixOS firewall
  networking.firewall.allowedUDPPorts = [ 67 ];

  # Configure network interfaces on the live ISO.
  # On PVE installs, fw_cfg parameters identify special interfaces by MAC:
  #   opt/nifty/mgmt_mac = mgmt interface (static IP only, no DHCP server)
  #   opt/nifty/wan_mac  = WAN interface (DHCP client, no static IP, no DHCP server)
  # All other interfaces get static IPs + DHCP server.
  # On bare metal (no fw_cfg), all interfaces get static IPs + DHCP server.
  systemd.services.nifty-live-network = {
    description = "Configure live ISO network interfaces";
    wantedBy = [ "multi-user.target" ];
    before = [ "nifty-live-dhcp.service" ];
    after = [ "systemd-udevd.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.gnugrep pkgs.dhcpcd ];
    script = ''
      MGMT_MAC=""
      WAN_MAC=""
      if [ -f /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw ]; then
        MGMT_MAC=$(cat /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw)
      fi
      if [ -f /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/wan_mac/raw ]; then
        WAN_MAC=$(cat /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/wan_mac/raw)
      fi

      # Get MAC for each interface
      get_mac() {
        ip -o link show "$1" 2>/dev/null | grep -oP 'link/ether \K[^ ]+'
      }

      SUBNET_BASE="10.99"
      INDEX=0
      for iface in $(ip -o link show | grep -oP '\d+: \K[^:]+' | grep -v '^lo$'); do
        ip link set "''${iface}" up
        MAC=$(get_mac "''${iface}")
        if [ -n "''${MGMT_MAC}" ] && [ "''${MAC}" = "''${MGMT_MAC}" ]; then
          # mgmt: static IP only (for PVE host SSH access)
          ip addr add ''${SUBNET_BASE}.''${INDEX}.1/24 dev "''${iface}" 2>/dev/null || true
        elif [ -n "''${WAN_MAC}" ] && [ "''${MAC}" = "''${WAN_MAC}" ]; then
          # WAN: run DHCP client (no static IP)
          dhcpcd -b "''${iface}" 2>/dev/null || true
        else
          # LAN: static IP (DHCP server added by nifty-live-dhcp)
          ip addr add ''${SUBNET_BASE}.''${INDEX}.1/24 dev "''${iface}" 2>/dev/null || true
        fi
        INDEX=$((INDEX + 1))
      done
    '';
  };

  systemd.services.nifty-live-dhcp = {
    description = "DHCP server for live ISO (LAN interfaces only)";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-live-network.service" ];
    serviceConfig = {
      ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq --keep-in-foreground --conf-file=/run/nifty-live-dnsmasq.conf";
      ExecStartPre = pkgs.writeShellScript "nifty-live-dhcp-config" ''
        PATH=${lib.makeBinPath [ pkgs.iproute2 pkgs.gnugrep pkgs.coreutils ]}

        MGMT_MAC=""
        WAN_MAC=""
        if [ -f /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw ]; then
          MGMT_MAC=$(cat /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/mgmt_mac/raw)
        fi
        if [ -f /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/wan_mac/raw ]; then
          WAN_MAC=$(cat /sys/firmware/qemu_fw_cfg/by_name/opt/nifty/wan_mac/raw)
        fi

        get_mac() {
          ip -o link show "$1" 2>/dev/null | grep -oP 'link/ether \K[^ ]+'
        }

        CONF=/run/nifty-live-dnsmasq.conf
        : > "''${CONF}"
        echo "bind-interfaces" >> "''${CONF}"
        echo "dhcp-leasefile=/tmp/dnsmasq.leases" >> "''${CONF}"
        echo "port=0" >> "''${CONF}"
        echo "dhcp-broadcast" >> "''${CONF}"
        echo "dhcp-option=option:dns-server,1.1.1.1,1.0.0.1" >> "''${CONF}"

        SUBNET_BASE="10.99"
        INDEX=0
        for iface in $(ip -o link show | grep -oP '\d+: \K[^:]+' | grep -v '^lo$'); do
          MAC=$(get_mac "''${iface}")
          if [ -n "''${MGMT_MAC}" ] && [ "''${MAC}" = "''${MGMT_MAC}" ]; then
            : # mgmt: no DHCP server
          elif [ -n "''${WAN_MAC}" ] && [ "''${MAC}" = "''${WAN_MAC}" ]; then
            : # WAN: no DHCP server
          else
            echo "interface=''${iface}" >> "''${CONF}"
            echo "dhcp-range=''${SUBNET_BASE}.''${INDEX}.100,''${SUBNET_BASE}.''${INDEX}.250,24h" >> "''${CONF}"
            echo "dhcp-option=tag:''${iface},option:router,''${SUBNET_BASE}.''${INDEX}.1" >> "''${CONF}"
          fi
          INDEX=$((INDEX + 1))
        done
      '';
      Restart = "on-failure";
    };
  };

  # Install script and tools available in PATH
  environment.systemPackages = with pkgs; [
    nifty-filter-pkg
    (writeShellScriptBin "nifty-install" ''exec nifty-filter install "$@"'')
    parted
    dosfstools
    e2fsprogs
    pciutils
    usbutils
    dhcpcd
  ];

  # Ship the default env file where the installer can find it
  environment.etc."nifty-filter/default-nifty-filter.env".source = ./default-nifty-filter.env;

  # Make the installed system closure available to the installer.
  # This is the disk-based system (with filesystem.nix), not the live ISO system.
  environment.etc."nifty-filter/installed-system".text = "${installedToplevel}";

  # Record which branch this ISO was built from so the installer
  # can write it to /var/nifty-filter/branch for nifty-upgrade.
  environment.etc."nifty-filter/build-branch".text = "${gitBranch}";

  # Include the installed system closure in the ISO's nix store
  isoImage.storeContents = [ installedToplevel ];

  # Allow console login for initial setup
  users.users.admin.initialPassword = lib.mkForce "nifty";
  services.openssh.settings.PasswordAuthentication = lib.mkForce true;

  # Use /etc/issue directly (writable on the live ISO)
  services.getty.extraArgs = lib.mkForce [ ];
  environment.etc."issue".text = lib.mkForce ''

    \e[1mnifty-filter\e[0m live installer (\n) \l
    \4

    Login:  admin / nifty

    Connect via SSH to install.

  '';

  users.motd = "";

  environment.interactiveShellInit = lib.mkForce ''
    export PS1='\[\e[1;32m\][LIVE ISO]\[\e[0m\] \u@\h:\w\$ '
    HOST_IP=$(${pkgs.iproute2}/bin/ip -4 addr show scope global 2>/dev/null | ${pkgs.gnugrep}/bin/grep -oP 'inet \K[0-9.]+' | head -1)
    HOST_IP=''${HOST_IP:-<this-host>}
    if [ -s "$HOME/.ssh/authorized_keys" ]; then
      echo ""
      echo "  SSH key installed. Ready to install."
      echo ""
      echo "   1. Run :"
      echo "        nifty-install"
      echo ""
    else
      echo ""
      echo "  Setup:"
      echo ""
      echo "   1. From your workstation, add your SSH public key:"
      echo "        ssh-copy-id admin@$HOST_IP"
      echo "          (password: nifty)"
      echo ""
      echo "   2. Connect from your workstation (using your key):"
      echo "        ssh admin@$HOST_IP"
      echo ""
      echo "  Once your key is installed, additional instructions will be given"
      echo "  when you connect."
      echo
    fi
  '';
}
