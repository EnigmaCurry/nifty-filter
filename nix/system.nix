# Read-only NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Router configuration: /var/nifty-filter/router.env
# To reconfigure: edit the env file and reboot.
{ config, pkgs, lib, scriptWizard ? null, ... }:

{
  system.stateVersion = "25.05";
  networking.hostName = "nifty-filter";

  # Boot (filesystem mounts are in filesystem.nix, not here,
  # so the ISO can provide its own without conflicts)
  boot.loader.systemd-boot.enable = lib.mkDefault true;
  boot.loader.efi.canTouchEfiVariables = lib.mkDefault false;
  boot.kernelPackages = pkgs.linuxPackages_latest;

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

  # --- Nifty-filter firewall (reads /var/nifty-filter/router.env at boot) ---
  services.nifty-filter.enable = true;

  # Set hostname from /var/nifty-filter/router.env at boot
  systemd.services.nifty-hostname = {
    description = "Set hostname from router.env";
    wantedBy = [ "sysinit.target" ];
    before = [ "network-pre.target" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig.Type = "oneshot";
    path = [ pkgs.hostname pkgs.gnugrep ];
    script = let d = "$"; in ''
      ENV_FILE="/var/nifty-filter/router.env"
      if [ -f "${d}ENV_FILE" ]; then
        NAME=${d}(grep -oP '^HOSTNAME=\K.*' "${d}ENV_FILE" || echo "")
        if [ -n "${d}NAME" ]; then
          hostname "${d}NAME"
        fi
      fi
    '';
  };

  # --- Networking ---
  # Interfaces are configured dynamically at boot from /var/nifty-filter/router.env
  # and /var/nifty-filter/dhcp.env. No hardcoded interface names.
  # Interface rename rules (.link files) are in /var/nifty-filter/network/
  networking.useDHCP = false;

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

  # Copy interface rename rules from /var early enough for udev
  systemd.services.nifty-link = {
    description = "Install interface rename rules";
    wantedBy = [ "sysinit.target" ];
    before = [ "systemd-udevd.service" "systemd-networkd.service" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if [ -d /var/nifty-filter/network ]; then
        mkdir -p /run/systemd/network
        cp /var/nifty-filter/network/*.link /run/systemd/network/ 2>/dev/null || true
      fi
    '';
  };

  # Configure WAN (DHCP) and LAN (static IP) from env files at boot
  systemd.services.nifty-network = {
    description = "Configure network interfaces from env files";
    wantedBy = [ "multi-user.target" ];
    before = [ "network.target" "nifty-filter.service" ];
    after = [ "network-pre.target" "nifty-filter-init.service" ];
    wants = [ "network-pre.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.systemd pkgs.gnugrep ];
    script = let d = "$"; in ''
      ENV_FILE="/var/nifty-filter/router.env"
      if [ ! -f "${d}ENV_FILE" ]; then
        echo "No router.env found, skipping network config"
        exit 0
      fi

      ENABLED=${d}(grep -oP '^ENABLED=\K.*' "${d}ENV_FILE" || echo "false")
      if [ "${d}ENABLED" != "true" ]; then
        echo "nifty-filter not enabled, skipping network config"
        exit 0
      fi

      INTERFACE_WAN=${d}(grep -oP '^INTERFACE_WAN=\K.*' "${d}ENV_FILE")
      INTERFACE_LAN=${d}(grep -oP '^INTERFACE_LAN=\K.*' "${d}ENV_FILE")
      ENABLE_IPV4=${d}(grep -oP '^ENABLE_IPV4=\K.*' "${d}ENV_FILE" || echo "true")
      ENABLE_IPV6=${d}(grep -oP '^ENABLE_IPV6=\K.*' "${d}ENV_FILE" || echo "false")

      # IPv4 subnet: prefer SUBNET_LAN_IPV4, fall back to SUBNET_LAN
      SUBNET_LAN_IPV4=${d}(grep -oP '^SUBNET_LAN_IPV4=\K.*' "${d}ENV_FILE" || grep -oP '^SUBNET_LAN=\K.*' "${d}ENV_FILE" || echo "")
      SUBNET_LAN_IPV6=${d}(grep -oP '^SUBNET_LAN_IPV6=\K.*' "${d}ENV_FILE" || echo "")

      # Bring up WAN with DHCP
      ip link set "${d}INTERFACE_WAN" up
      mkdir -p /run/systemd/network

      WAN_NETWORK="[Network]"
      if [ "${d}ENABLE_IPV4" = "true" ]; then
        WAN_NETWORK="${d}WAN_NETWORK
      DHCP=ipv4"
      fi
      if [ "${d}ENABLE_IPV6" = "true" ]; then
        WAN_NETWORK="${d}WAN_NETWORK
      IPv6AcceptRA=yes"
      fi

      cat > /run/systemd/network/10-wan.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_WAN

      ${d}WAN_NETWORK

      [DHCPv4]
      UseDNS=yes

      [IPv6AcceptRA]
      UseDNS=yes
      NETEOF

      # Bring up LAN with static IP(s)
      ip link set "${d}INTERFACE_LAN" up

      LAN_NETWORK="[Network]"
      if [ "${d}ENABLE_IPV4" = "true" ] && [ -n "${d}SUBNET_LAN_IPV4" ]; then
        LAN_NETWORK="${d}LAN_NETWORK
      Address=${d}SUBNET_LAN_IPV4"
      fi
      if [ "${d}ENABLE_IPV6" = "true" ] && [ -n "${d}SUBNET_LAN_IPV6" ]; then
        LAN_NETWORK="${d}LAN_NETWORK
      Address=${d}SUBNET_LAN_IPV6
      IPv6SendRA=yes"
      fi

      cat > /run/systemd/network/10-lan.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_LAN

      ${d}LAN_NETWORK
      NETEOF

      # Add IPv6 prefix for Router Advertisements if IPv6 is enabled on LAN
      if [ "${d}ENABLE_IPV6" = "true" ] && [ -n "${d}SUBNET_LAN_IPV6" ]; then
        cat >> /run/systemd/network/10-lan.network <<NETEOF

      [IPv6SendRA]
      Managed=no
      OtherInformation=no

      [IPv6Prefix]
      Prefix=${d}SUBNET_LAN_IPV6
      NETEOF
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
  # Config is generated at boot from /var/nifty-filter/dhcp.env
  # Forwards DNS to upstream (Cloudflare by default).
  services.resolved.enable = false;

  systemd.services.nifty-dnsmasq = {
    description = "dnsmasq DHCP and DNS server from env file";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" ];
    serviceConfig = {
      Type = "forking";
      PIDFile = "/run/dnsmasq.pid";
      ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq -C /run/dnsmasq.conf";
      ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
      Restart = "on-failure";
    };
    path = [ pkgs.gnugrep pkgs.gnused pkgs.coreutils ];
    preStart = let d = "$"; in ''
      DHCP_ENV="/var/nifty-filter/dhcp.env"
      if [ ! -f "${d}DHCP_ENV" ]; then
        echo "No dhcp.env found, writing minimal DNS-only config"
        cat > /run/dnsmasq.conf <<DNSEOF
      pid-file=/run/dnsmasq.pid
      listen-address=127.0.0.1
      bind-interfaces
      no-resolv
      server=1.1.1.1
      server=1.0.0.1
      DNSEOF
        exit 0
      fi

      DHCP_INTERFACE=${d}(grep -oP '^DHCP_INTERFACE=\K.*' "${d}DHCP_ENV")
      DHCP_SUBNET=${d}(grep -oP '^DHCP_SUBNET=\K.*' "${d}DHCP_ENV")
      DHCP_POOL_START=${d}(grep -oP '^DHCP_POOL_START=\K.*' "${d}DHCP_ENV")
      DHCP_POOL_END=${d}(grep -oP '^DHCP_POOL_END=\K.*' "${d}DHCP_ENV")
      DHCP_ROUTER=${d}(grep -oP '^DHCP_ROUTER=\K.*' "${d}DHCP_ENV")
      DHCP_DNS=${d}(grep -oP '^DHCP_DNS=\K.*' "${d}DHCP_ENV" || echo "1.1.1.1, 1.0.0.1")

      IFS='/' read -r ROUTER_IP PREFIX <<< "${d}DHCP_SUBNET"

      # Build upstream server lines from DNS list
      DNS_SERVERS=""
      IFS=',' read -ra DNS_ARRAY <<< "${d}DHCP_DNS"
      for dns in "${d}{DNS_ARRAY[@]}"; do
        dns=${d}(echo "${d}dns" | tr -d ' ')
        DNS_SERVERS="${d}{DNS_SERVERS}
      server=${d}dns"
      done

      mkdir -p /var/lib/dnsmasq
      cat > /run/dnsmasq.conf <<DNSEOF
      # Generated from /var/nifty-filter/dhcp.env
      pid-file=/run/dnsmasq.pid

      # DNS
      no-resolv
      ${d}DNS_SERVERS
      domain-needed
      bogus-priv
      cache-size=1000

      # Listen on LAN interface and localhost
      interface=${d}DHCP_INTERFACE
      listen-address=${d}ROUTER_IP
      listen-address=127.0.0.1
      bind-interfaces

      # DHCP
      dhcp-range=${d}DHCP_POOL_START,${d}DHCP_POOL_END,24h
      dhcp-option=option:router,${d}DHCP_ROUTER
      dhcp-option=option:dns-server,${d}ROUTER_IP
      dhcp-leasefile=/var/lib/dnsmasq/dnsmasq.leases

      # Logging
      log-dhcp
      DNSEOF
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
    (writeShellScriptBin "nifty-config" (builtins.readFile ./nifty-config.sh))
    (writeShellScriptBin "nifty-maintenance" (builtins.readFile ./nifty-maintenance.sh))
    (writeShellScriptBin "nifty-upgrade" (builtins.readFile ./nifty-upgrade.sh))
    git
  ] ++ lib.optional (scriptWizard != null) scriptWizard ++ [
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

  # Auto-login on console in maintenance mode only
  systemd.services."getty@tty1" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.bash}/bin/bash -c 'if grep -q nifty.maintenance=1 /proc/cmdline; then exec ${pkgs.shadow}/bin/login -f admin; else exec ${pkgs.util-linux}/bin/agetty --noclear --keep-baud tty1 115200,38400,9600 linux; fi'"
    ];
  };
  # Pre-login banner using agetty built-in escapes (works on read-only root)
  environment.etc."issue".text = lib.mkDefault ''

    \e[1mnifty-filter\e[0m (\n) \l
    wan: \4{wan}
    lan: \4{lan}

  '';

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
}
