# Immutable NixOS router system
#
# Root filesystem is read-only. All mutable state lives on /var.
# Router configuration: /var/nifty-filter/router.env
# To reconfigure: edit the env file and reboot.
{ config, pkgs, lib, ... }:

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
  networking.useDHCP = false;

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

  # --- DHCP server for LAN clients ---
  # Kea config is generated at boot from /var/nifty-filter/dhcp.env
  systemd.services.nifty-dhcp = {
    description = "Configure and start DHCP server from env file";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-network.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.kea pkgs.coreutils pkgs.gnugrep pkgs.gnused ];
    script = let d = "$"; in ''
      DHCP_ENV="/var/nifty-filter/dhcp.env"
      if [ ! -f "${d}DHCP_ENV" ]; then
        echo "No dhcp.env found, skipping DHCP server"
        exit 0
      fi

      DHCP_INTERFACE=${d}(grep -oP '^DHCP_INTERFACE=\K.*' "${d}DHCP_ENV")
      DHCP_SUBNET=${d}(grep -oP '^DHCP_SUBNET=\K.*' "${d}DHCP_ENV")
      DHCP_POOL_START=${d}(grep -oP '^DHCP_POOL_START=\K.*' "${d}DHCP_ENV")
      DHCP_POOL_END=${d}(grep -oP '^DHCP_POOL_END=\K.*' "${d}DHCP_ENV")
      DHCP_ROUTER=${d}(grep -oP '^DHCP_ROUTER=\K.*' "${d}DHCP_ENV")
      DHCP_DNS=${d}(grep -oP '^DHCP_DNS=\K.*' "${d}DHCP_ENV")

      # Derive network address from subnet CIDR
      IFS='/' read -r ROUTER_IP PREFIX <<< "${d}DHCP_SUBNET"
      NETWORK_BASE=${d}(echo "${d}ROUTER_IP" | sed 's/\.[0-9]*${d}//')
      NETWORK="${d}{NETWORK_BASE}.0/${d}{PREFIX}"

      mkdir -p /run/kea
      cat > /run/kea/kea-dhcp4.conf <<KEAEOF
      {
        "Dhcp4": {
          "interfaces-config": { "interfaces": ["${d}DHCP_INTERFACE"] },
          "lease-database": {
            "type": "memfile",
            "name": "/var/lib/kea/dhcp4.leases",
            "persist": true
          },
          "subnet4": [{
            "id": 1,
            "subnet": "${d}NETWORK",
            "pools": [{ "pool": "${d}DHCP_POOL_START - ${d}DHCP_POOL_END" }],
            "option-data": [
              { "name": "routers", "data": "${d}DHCP_ROUTER" },
              { "name": "domain-name-servers", "data": "${d}DHCP_DNS" }
            ]
          }]
        }
      }
      KEAEOF

      mkdir -p /var/lib/kea
      kea-dhcp4 -c /run/kea/kea-dhcp4.conf &
    '';
  };

  # --- DNS resolver ---
  services.resolved = {
    enable = true;
    settings.Resolve.FallbackDNS = [ "1.1.1.1" "1.0.0.1" ];
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
    vim
    htop
    ethtool
    tcpdump
    iproute2
    dig
  ];

  # Pre-login banner with interface IPs
  systemd.services.update-issue = {
    description = "Generate /etc/issue with interface IPs";
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
        echo -e "  \e[1mnifty-filter\e[0m"
        echo ""
        ip -4 -o addr show scope global | awk '{printf "  %-12s %s\n", $2, $4}'
        echo ""
      } > /run/issue
      ln -sf /run/issue /etc/issue
    '';
  };

  # Keep it lean
  documentation.enable = false;
  services.xserver.enable = false;
}
