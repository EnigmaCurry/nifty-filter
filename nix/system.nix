# Immutable NixOS router system
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

  # Disable nix operations on the immutable system
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  nix.gc.automatic = false;

  # --- Nifty-filter firewall (reads /var/nifty-filter/router.env at boot) ---
  services.nifty-filter.enable = true;

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
      ExecStart = "${pkgs.util-linux}/bin/mount -o remount,rw /nix/store";
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
      SUBNET_LAN=${d}(grep -oP '^SUBNET_LAN=\K.*' "${d}ENV_FILE")

      # Bring up WAN with DHCP
      ip link set "${d}INTERFACE_WAN" up
      mkdir -p /run/systemd/network
      cat > /run/systemd/network/10-wan.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_WAN

      [Network]
      DHCP=ipv4

      [DHCPv4]
      UseDNS=yes
      NETEOF

      # Bring up LAN with static IP
      ip link set "${d}INTERFACE_LAN" up
      cat > /run/systemd/network/10-lan.network <<NETEOF
      [Match]
      Name=${d}INTERFACE_LAN

      [Network]
      Address=${d}SUBNET_LAN
      NETEOF

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

  # --- Minimal packages ---
  environment.systemPackages = with pkgs; [
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
      echo "  Upgrade system:  sudo nifty-upgrade"
      echo "  Return to normal: sudo reboot"
      echo ""
    fi
  '';

  # Auto-login on console in maintenance mode only
  systemd.services."getty@tty1" = {
    overrideStrategy = "asDropin";
    serviceConfig.ExecStart = lib.mkForce [
      ""  # clear default
      "${pkgs.bash}/bin/bash -c 'if grep -q nifty.maintenance=1 /proc/cmdline; then exec ${pkgs.shadow}/bin/login -f admin; else exec ${pkgs.util-linux}/bin/agetty --issue-file /run/issue --noclear --keep-baud tty1 115200,38400,9600 linux; fi'"
    ];
  };

  # Pre-login banner with interface IPs (written to /run since / is read-only)
  services.getty.extraArgs = [ "--issue-file" "/run/issue" ];
  systemd.services.update-issue = {
    description = "Generate /run/issue with interface IPs";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.iproute2 pkgs.gawk ];
    script = ''
      {
        echo ""
        if grep -q 'nifty.maintenance=1' /proc/cmdline 2>/dev/null; then
          echo -e "  \e[1;31m*** nifty-filter MAINTENANCE MODE ***\e[0m"
        else
          echo -e "  \e[1mnifty-filter\e[0m"
        fi
        echo ""
        ip -4 -o addr show scope global | awk '{printf "  %-12s %s\n", $2, $4}'
        echo ""
      } > /run/issue
    '';
  };

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
}
