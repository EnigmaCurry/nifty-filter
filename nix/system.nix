# Read-only NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Configuration: /var/nifty-filter/nifty-filter.hcl
# To reconfigure: edit the HCL file and reboot
{ config, pkgs, lib, ... }:

let
  hclFile = "/var/nifty-filter/nifty-filter.hcl";
in
{
  system.stateVersion = "25.05";
  networking.hostName = "nifty-filter";

  # Boot (filesystem mounts are in filesystem.nix, not here,
  # so the ISO can provide its own without conflicts)
  boot.loader.systemd-boot.enable = lib.mkDefault true;
  boot.loader.efi.canTouchEfiVariables = lib.mkDefault false;
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Serial console support (works alongside VGA)
  boot.kernelParams = [ "console=tty0" "console=ttyS0,115200n8" ];

  # Include common disk/filesystem drivers in initrd
  boot.initrd.availableKernelModules = [
    # Virtio (QEMU/KVM)
    "virtio_pci" "virtio_blk" "virtio_scsi" "virtio_net"
    # SATA/AHCI
    "ahci" "sd_mod"
    # NVMe
    "nvme"
    # USB storage
    "usb_storage" "uas" "xhci_pci" "ehci_pci"
    # SCSI
    "sr_mod"
  ];

  # Disable nix operations on the read-only system
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  nix.gc.automatic = false;

  # --- Nifty-filter firewall (reads /var/nifty-filter/nifty-filter.hcl at boot) ---
  services.nifty-filter.enable = true;
  services.nifty-filter.packages.sodola-switch.enable = true;
  services.nifty-filter.packages.nifty-dashboard.enable = true;
  services.nifty-filter.packages.iperf.enable = true;

  # Set hostname from HCL config at boot
  systemd.services.nifty-hostname = {
    description = "Set hostname from nifty-filter HCL config";
    wantedBy = [ "sysinit.target" ];
    before = [ "network-pre.target" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.hostname ];
    script = ''
      if [ -f ${hclFile} ]; then
        NAME=$(nifty-filter hostname --config ${hclFile} 2>/dev/null)
        if [ -n "$NAME" ]; then
          hostname "$NAME"
        fi
      fi
    '';
  };

  # --- Networking ---
  # Interfaces are configured dynamically at boot from /var/nifty-filter/nifty-filter.env
  # No hardcoded interface names.
  # Interface rename rules (.link files) are in /var/nifty-filter/network/
  networking.useDHCP = false;

  # Reverse-path filtering (BCP 38 / RFC 3704)
  # Strict mode (1): drop packets whose source address would not be routed
  # back out the same interface they arrived on.
  boot.kernel.sysctl = {
    "net.ipv4.conf.default.rp_filter" = 1;
    "net.ipv4.conf.all.rp_filter" = 1;
  };

  # Remount root read-only after NixOS activation completes
  systemd.services.nifty-ro = {
    description = "Remount root filesystem read-only";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-tmpfiles-setup.service" ];
    unitConfig.ConditionKernelCommandLine = "!nifty.maintenance=1";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if ${pkgs.util-linux}/bin/findmnt -n -t ext4 / > /dev/null 2>&1; then
        ${pkgs.util-linux}/bin/mount -o remount,ro /
      fi
    '';
  };

  # In maintenance mode, remount /nix/store as read-write
  systemd.services.nifty-maintenance-rw = {
    description = "Remount nix store read-write in maintenance mode";
    wantedBy = [ "sysinit.target" ];
    before = [ "nix-daemon.service" ];
    unitConfig.DefaultDependencies = false;
    unitConfig.ConditionKernelCommandLine = "nifty.maintenance=1";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = [
        "${pkgs.util-linux}/bin/mount -o remount,rw /"
        "${pkgs.util-linux}/bin/mount -o remount,rw /nix/store"
      ];
    };
  };

  # Generate interface rename rules (.link files) from HCL config at boot
  systemd.services.nifty-link = {
    description = "Generate interface rename rules from HCL config";
    wantedBy = [ "sysinit.target" ];
    before = [ "systemd-udevd.service" "systemd-networkd.service" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if [ ! -f ${hclFile} ]; then
        echo "No HCL config found, skipping link generation"
        exit 0
      fi
      mkdir -p /run/systemd/network
      nifty-filter generate linkfiles --config ${hclFile} --output-dir /run/systemd/network
    '';
  };

  # Configure WAN, trunk/VLANs, and optional mgmt from HCL config at boot
  # nifty-network is the sole owner of runtime .network and .netdev files.
  systemd.services.nifty-network = {
    description = "Configure network interfaces from HCL config";
    wantedBy = [ "multi-user.target" ];
    before = [ "network.target" "nifty-filter.service" ];
    after = [ "network-pre.target" "nifty-filter-init.service" ];
    wants = [ "network-pre.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.systemd pkgs.procps pkgs.gnugrep ];
    script = ''
      if [ ! -f ${hclFile} ]; then
        echo "No HCL config found, skipping network config"
        exit 0
      fi

      # Read interface names from HCL
      WAN_INTERFACE=$(grep -oP 'wan\s*=\s*"\K[^"]+' ${hclFile} | head -1)
      TRUNK_INTERFACE=$(grep -oP 'trunk\s*=\s*"\K[^"]+' ${hclFile} | head -1)
      MGMT_INTERFACE=$(grep -oP 'mgmt\s*=\s*"\K[^"]+' ${hclFile} | head -1)
      ENABLE_IPV6=$(grep -oP 'enable_ipv6\s*=\s*\K\w+' ${hclFile} | head -1)

      # Bring up interfaces
      [ -n "$WAN_INTERFACE" ] && ip link set "$WAN_INTERFACE" up
      [ -n "$TRUNK_INTERFACE" ] && ip link set "$TRUNK_INTERFACE" up
      [ -n "$MGMT_INTERFACE" ] && ip link set "$MGMT_INTERFACE" up

      # Generate networkd config files
      mkdir -p /run/systemd/network
      nifty-filter generate networkd --config ${hclFile} --output-dir /run/systemd/network

      # Ensure WAN accepts RAs despite forwarding (must override after networkd)
      if [ "$ENABLE_IPV6" = "true" ] && [ -n "$WAN_INTERFACE" ]; then
        mkdir -p /etc/systemd/system/systemd-networkd.service.d
        cat > /etc/systemd/system/systemd-networkd.service.d/accept-ra.conf <<RAEOF
      [Service]
      ExecStartPost=/bin/sh -c 'sleep 1 && /run/current-system/sw/bin/sysctl -w net.ipv6.conf.$WAN_INTERFACE.accept_ra=2 net.ipv6.conf.$WAN_INTERFACE.forwarding=0'
      RAEOF
        systemctl daemon-reload
      fi

      # Disable IPv6 on management interface
      if [ -n "$MGMT_INTERFACE" ]; then
        sysctl -w net.ipv6.conf.$MGMT_INTERFACE.disable_ipv6=1
      fi

      # Restart networkd to pick up the new configs
      networkctl reload || systemctl restart systemd-networkd
    '';
  };

  # Use systemd-networkd for runtime network config
  networking.useNetworkd = true;

  # Static resolv.conf pointing to local dnsmasq (no resolvconf needed)
  networking.resolvconf.enable = false;
  environment.etc."resolv.conf".text = ''
    nameserver 127.0.0.1
  '';

  # --- dnsmasq: DHCP + DNS for LAN ---
  # Config is generated at boot from /var/nifty-filter/nifty-filter.hcl
  # Forwards DNS to upstream (Cloudflare by default).
  services.resolved.enable = false;

  systemd.services.nifty-dnsmasq = {
    description = "dnsmasq DHCP and DNS server";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" ];
    serviceConfig = {
      Type = "forking";
      PIDFile = "/run/dnsmasq.pid";
      ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq -C /run/dnsmasq.conf";
      ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
      Restart = "on-failure";
    };
    preStart = ''
      mkdir -p /var/lib/dnsmasq
      if [ -f ${hclFile} ]; then
        nifty-filter generate dnsmasq --config ${hclFile} --output /run/dnsmasq.conf
      else
        nifty-filter generate dnsmasq-minimal --output /run/dnsmasq.conf
      fi
    '';
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
  # SSH keys are provisioned at runtime from /var, not at build time.
  # The installer ensures keys exist before writing to disk.
  users.allowNoPasswordLogin = true;
  users.mutableUsers = false;
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    # SSH authorized keys live in ~/.ssh/authorized_keys (standard path).
    # Since /home is bind-mounted from /var/home, this persists across reboots.
    # Use ssh-copy-id to add keys.
  };
  security.sudo.wheelNeedsPassword = false;

  # Allow wheel users to reboot/poweroff without sudo
  security.polkit.extraConfig = ''
    polkit.addRule(function(action, subject) {
      if ((action.id == "org.freedesktop.login1.reboot" ||
           action.id == "org.freedesktop.login1.reboot-multiple-sessions" ||
           action.id == "org.freedesktop.login1.power-off" ||
           action.id == "org.freedesktop.login1.power-off-multiple-sessions") &&
          subject.isInGroup("wheel")) {
        return polkit.Result.YES;
      }
    });
  '';

  # --- Minimal packages ---
  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "nifty-config" ''exec nifty-filter config "$@"'')
    (writeShellScriptBin "nifty-maintenance" ''exec nifty-filter maintenance "$@"'')
    (writeShellScriptBin "nifty-upgrade" ''exec nifty-filter upgrade "$@"'')
    git
    vim
    htop
    ethtool
    tcpdump
    iproute2
    dig
  ];

  # Maintenance mode indicator — modify PS1 and show warning
  environment.interactiveShellInit = ''
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
      echo "  Configure:  nifty-config"
      echo "  Upgrade:    nifty-upgrade"
      echo ""
    fi
  '';

  # Auto-login on console (SSH is primary access; console is for emergencies)
  systemd.services."getty@tty1" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.util-linux}/bin/agetty --autologin admin --noclear --keep-baud %I 115200,38400,9600 $TERM"
    ];
  };
  # Serial console auto-login
  systemd.services."serial-getty@ttyS0" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.util-linux}/bin/agetty --autologin admin --keep-baud ttyS0 115200,38400,9600 vt100"
    ];
  };
  # Pre-login banner using agetty built-in escapes (works on read-only root)
  environment.etc."issue".text = lib.mkDefault ''

    \e[1mnifty-filter\e[0m (\n) \l
    wan: \4{wan}
    trunk: \4{trunk}

  '';

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
}
