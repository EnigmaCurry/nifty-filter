#!/usr/bin/env bash
# nifty-install — install nifty-filter to disk from the live ISO
#
# Uses script-wizard for interactive configuration:
#   - Target disk selection
#   - WAN/LAN interface selection
#   - LAN subnet and DHCP pool configuration
#
# Prerequisites:
#   1. ssh-copy-id admin@<host>
#   2. Reconnect over SSH using key auth
#   3. Run this installer
set -euo pipefail

GIT_REMOTE=""
AUTH_KEYS="/home/admin/.ssh/authorized_keys"

usage() {
    cat <<EOF
Usage: nifty-install [options]

Install nifty-filter to a disk from the running live ISO.
Interactively selects disk, network interfaces, and DHCP settings.

Before running, you must:
  1. Add your SSH public key:
       ssh-copy-id admin@<this-host>
  2. Reconnect using key authentication

Options:
  --git-remote URL  Set a git remote for config updates
  -h, --help        Show this help
EOF
    exit "${1:-0}"
}

die() { echo "ERROR: $*" >&2; exit 1; }

# Check if the current SSH session is using key authentication.
check_ssh_auth() {
    # Not over SSH — console is fine
    if [[ -z "${SSH_CONNECTION:-}" ]]; then
        return 0
    fi

    # Find the sshd process for this session
    local sshd_pid
    sshd_pid=$(ps -o ppid= -p $$ | tr -d ' ')
    while [[ "$sshd_pid" -gt 1 ]]; do
        local cmd
        cmd=$(ps -o comm= -p "$sshd_pid" 2>/dev/null || true)
        if [[ "$cmd" == "sshd" ]]; then
            break
        fi
        sshd_pid=$(ps -o ppid= -p "$sshd_pid" 2>/dev/null | tr -d ' ')
    done

    local auth_method
    auth_method=$(journalctl _PID="$sshd_pid" -o cat --no-pager 2>/dev/null | grep -oP 'Accepted \K\S+' | head -1)

    if [[ "$auth_method" == "password" ]]; then
        echo ""
        echo "REFUSED: You are connected via password authentication."
        echo ""
        echo "The installer requires SSH key authentication so that"
        echo "trust is established before writing to disk."
        echo ""
        echo "Steps:"
        echo "  1. Add your public key (from your workstation):"
        echo "       ssh-copy-id admin@<this-host>"
        echo "  2. Disconnect and reconnect with your key:"
        echo "       ssh admin@<this-host>"
        echo "  3. Run this installer again"
        echo ""
        exit 1
    elif [[ "$auth_method" == "publickey" ]]; then
        return 0
    else
        echo "WARNING: Could not determine SSH auth method (method=$auth_method)."
        echo "         Proceeding anyway."
        return 0
    fi
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --git-remote) GIT_REMOTE="$2"; shift 2 ;;
        -h|--help) usage 0 ;;
        -*) die "Unknown option: $1" ;;
        *) die "Unexpected argument: $1" ;;
    esac
done

[[ $EUID -eq 0 ]] || exec sudo "$0" "$@"

# --- Pre-flight checks ---

if [[ ! -s "$AUTH_KEYS" ]]; then
    echo ""
    echo "REFUSED: No SSH authorized keys found."
    echo ""
    echo "You must add at least one SSH public key before installing."
    echo "This key will be carried into the installed system."
    echo ""
    echo "  ssh-copy-id admin@<this-host>"
    echo ""
    echo "Then run this installer again."
    echo ""
    exit 1
fi

echo "==> Checking SSH authentication method..."
check_ssh_auth
echo "  OK: key authentication confirmed"

echo ""
echo "==> Authorized keys that will be installed:"
while IFS= read -r key; do
    [[ -z "$key" || "$key" == \#* ]] && continue
    echo "  $(echo "$key" | awk '{print $1, $NF}')"
done < "$AUTH_KEYS"
echo ""

# --- Interactive configuration with script-wizard ---

# Hostname
echo "==> Configure hostname:"
while true; do
    HOSTNAME=$(script-wizard ask "Hostname for this router" "nifty-filter")
    if [[ "$HOSTNAME" =~ ^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$ ]]; then
        break
    fi
    echo "  Invalid hostname. Must be 1-63 characters: letters, digits, hyphens. Cannot start/end with hyphen."
done
echo "  Hostname: $HOSTNAME"
echo ""

# Select target disk
echo "==> Select target disk for installation:"
DISKS=()
while read -r name size model; do
    DISKS+=("${name} (${size} ${model})")
done < <(lsblk -ndo NAME,SIZE,MODEL -e 7,11 | grep -v '^loop')

if [[ ${#DISKS[@]} -eq 0 ]]; then
    die "No disks found"
elif [[ ${#DISKS[@]} -eq 1 ]]; then
    DISK_CHOICE="${DISKS[0]}"
    echo "  Only one disk found: $DISK_CHOICE"
else
    DISK_CHOICE=$(script-wizard choose "Select target disk:" "${DISKS[@]}")
fi
DISK="/dev/$(echo "$DISK_CHOICE" | awk '{print $1}')"
echo "  Selected: $DISK"

# Select network interfaces
echo ""
echo "==> Configure network interfaces:"
IFACES=()
while read -r name; do
    [[ "$name" == "lo" ]] && continue
    IFACES+=("$name")
done < <(ip -o link show | awk -F': ' '{print $2}')

if [[ ${#IFACES[@]} -lt 2 ]]; then
    die "Need at least 2 network interfaces (found ${#IFACES[@]})"
fi

REAL_WAN=$(script-wizard choose "Select WAN interface (upstream/internet):" "${IFACES[@]}")
echo "  WAN: $REAL_WAN -> will be renamed to 'wan'"

# Remove WAN from choices for LAN
LAN_IFACES=()
for iface in "${IFACES[@]}"; do
    [[ "$iface" != "$REAL_WAN" ]] && LAN_IFACES+=("$iface")
done

if [[ ${#LAN_IFACES[@]} -eq 1 ]]; then
    REAL_LAN="${LAN_IFACES[0]}"
    echo "  LAN: $REAL_LAN -> will be renamed to 'lan' (only remaining interface)"
else
    REAL_LAN=$(script-wizard choose "Select LAN interface (local network):" "${LAN_IFACES[@]}")
    echo "  LAN: $REAL_LAN -> will be renamed to 'lan'"
fi

# Get MAC addresses for persistent renaming
WAN_MAC=$(ip -o link show "$REAL_WAN" | grep -oP 'link/ether \K[^ ]+')
LAN_MAC=$(ip -o link show "$REAL_LAN" | grep -oP 'link/ether \K[^ ]+')

# Use canonical names
INTERFACE_WAN="wan"
INTERFACE_LAN="lan"

# Configure LAN subnet
echo ""
echo "==> Configure LAN network:"
SUBNET_LAN=$(script-wizard ask "LAN subnet (router IP/prefix)" "10.99.0.1/24")
echo "  Subnet: $SUBNET_LAN"

# Extract network info for DHCP defaults
ROUTER_IP=$(echo "$SUBNET_LAN" | cut -d/ -f1)
PREFIX=$(echo "$SUBNET_LAN" | cut -d/ -f2)
# Derive base network (simple: replace last octet)
NETWORK_BASE=$(echo "$ROUTER_IP" | sed 's/\.[0-9]*$//')
DHCP_START="${NETWORK_BASE}.100"
DHCP_END="${NETWORK_BASE}.250"

echo ""
echo "==> Configure DHCP pool:"
DHCP_START=$(script-wizard ask "DHCP pool start" "$DHCP_START")
DHCP_END=$(script-wizard ask "DHCP pool end" "$DHCP_END")
echo "  Pool: $DHCP_START - $DHCP_END"

DNS_SERVERS=$(script-wizard ask "DNS servers for DHCP clients" "1.1.1.1, 1.0.0.1")
echo "  DNS: $DNS_SERVERS"

# --- Confirm ---
echo ""
echo "==> Installation summary:"
echo "  Hostname:     $HOSTNAME"
echo "  Disk:         $DISK"
echo "  WAN:          $REAL_WAN ($WAN_MAC) -> wan"
echo "  LAN:          $REAL_LAN ($LAN_MAC) -> lan"
echo "  LAN subnet:   $SUBNET_LAN"
echo "  DHCP pool:    $DHCP_START - $DHCP_END"
echo "  DNS servers:  $DNS_SERVERS"
if [[ -n "$GIT_REMOTE" ]]; then
echo "  Git remote:   $GIT_REMOTE"
fi
echo ""
echo "  WARNING: This will ERASE ALL DATA on $DISK"
echo ""

script-wizard confirm "Proceed with installation?" || { echo "Aborted."; exit 1; }

MNT=$(mktemp -d)
trap 'umount -R "$MNT" 2>/dev/null || true; rmdir "$MNT" 2>/dev/null || true' EXIT

echo ""
echo "==> Partitioning $DISK..."
wipefs -af "$DISK"
parted -s "$DISK" \
    mklabel gpt \
    mkpart NIFTY_BOOT fat32 1MiB 513MiB \
    set 1 esp on \
    mkpart NIFTY_ROOT ext4 513MiB 8705MiB \
    mkpart NIFTY_VAR ext4 8705MiB 100%

udevadm settle
sleep 1

# Detect partition paths
if [[ "$DISK" == *nvme* ]] || [[ "$DISK" == *mmcblk* ]]; then
    PART_BOOT="${DISK}p1"
    PART_ROOT="${DISK}p2"
    PART_VAR="${DISK}p3"
else
    PART_BOOT="${DISK}1"
    PART_ROOT="${DISK}2"
    PART_VAR="${DISK}3"
fi

echo "==> Formatting partitions..."
mkfs.vfat -F 32 -n NIFTY_BOOT "$PART_BOOT"
mkfs.ext4 -F -L NIFTY_ROOT -q "$PART_ROOT"
mkfs.ext4 -F -L NIFTY_VAR -q "$PART_VAR"

echo "==> Mounting filesystems..."
mount "$PART_ROOT" "$MNT"
mkdir -p "$MNT/boot" "$MNT/var"
mount "$PART_BOOT" "$MNT/boot"
mount "$PART_VAR" "$MNT/var"

echo "==> Copying system closure to disk..."
SYSTEM_PATH=$(cat /etc/nifty-filter/installed-system)
echo "  System: $SYSTEM_PATH"

mkdir -p "$MNT/nix/store"
nix-store -qR "$SYSTEM_PATH" | while read -r path; do
    echo "  copying $(basename "$path")"
    cp -a "$path" "$MNT/nix/store/"
done

mkdir -p "$MNT/nix/var/nix/db"
nix-store --dump-db | nix-store --load-db --store "$MNT"

echo "==> Setting up system profile..."
mkdir -p "$MNT/nix/var/nix/profiles"
ln -sfn "$SYSTEM_PATH" "$MNT/nix/var/nix/profiles/system"

echo "==> Installing bootloader..."
bootctl install --esp-path="$MNT/boot"

KERNEL=$(readlink -f "$SYSTEM_PATH/kernel")
INITRD=$(readlink -f "$SYSTEM_PATH/initrd")
echo "  Kernel: $KERNEL"
echo "  Initrd: $INITRD"

cp "$KERNEL" "$MNT/boot/kernel"
cp "$INITRD" "$MNT/boot/initrd"

KERNEL_PARAMS=$(cat "$SYSTEM_PATH/kernel-params" 2>/dev/null || echo "")

mkdir -p "$MNT/boot/loader"
cat > "$MNT/boot/loader/loader.conf" <<LOADER
default nifty-filter.conf
timeout 3
editor no
LOADER

mkdir -p "$MNT/boot/loader/entries"
cat > "$MNT/boot/loader/entries/nifty-filter.conf" <<ENTRY
title   nifty-filter
linux   /kernel
initrd  /initrd
options init=$SYSTEM_PATH/init $KERNEL_PARAMS
ENTRY

cat > "$MNT/boot/loader/entries/nifty-filter-maintenance.conf" <<ENTRY
title   nifty-filter (maintenance)
linux   /kernel
initrd  /initrd
options init=$SYSTEM_PATH/init $KERNEL_PARAMS rw nifty.maintenance=1
ENTRY
echo "  Boot entries created"

echo "==> Setting up /var..."
mkdir -p "$MNT/var/nifty-filter/ssh"
mkdir -p "$MNT/var/nifty-filter/network"
mkdir -p "$MNT/var/home/admin/.ssh"
mkdir -p "$MNT/var/root"
mkdir -p "$MNT/var/log/journal"

# Create systemd.link files to rename interfaces by MAC address
echo "==> Creating interface rename rules..."
cat > "$MNT/var/nifty-filter/network/10-wan.link" <<LINKEOF
[Match]
MACAddress=$WAN_MAC

[Link]
Name=wan
LINKEOF

cat > "$MNT/var/nifty-filter/network/10-lan.link" <<LINKEOF
[Match]
MACAddress=$LAN_MAC

[Link]
Name=lan
LINKEOF

# Write router config with user's choices
cat > "$MNT/var/nifty-filter/router.env" <<ENVEOF
# nifty-filter router configuration
# Edit this file and systemctl reboot to apply changes.
#
# This file lives on the writable /var partition.
# The rest of the system is immutable (unless booted in maintenance mode).
ENABLED=true
HOSTNAME=${HOSTNAME}

# Network interfaces
INTERFACE_LAN=${INTERFACE_LAN}
INTERFACE_WAN=${INTERFACE_WAN}

# LAN subnet in CIDR notation (router's LAN IP / prefix length)
SUBNET_LAN=${SUBNET_LAN}

# ICMP types accepted on each interface
ICMP_ACCEPT_LAN=echo-request,echo-reply,destination-unreachable,time-exceeded
ICMP_ACCEPT_WAN=

# TCP/UDP ports the router itself accepts
TCP_ACCEPT_LAN=22
UDP_ACCEPT_LAN=
TCP_ACCEPT_WAN=22
UDP_ACCEPT_WAN=

# Port forwarding rules
# Format: incoming_port:destination_ip:destination_port
TCP_FORWARD_LAN=
UDP_FORWARD_LAN=
TCP_FORWARD_WAN=
UDP_FORWARD_WAN=
ENVEOF
chmod 0600 "$MNT/var/nifty-filter/router.env"

# Write DHCP config for the init service to pick up
cat > "$MNT/var/nifty-filter/dhcp.env" <<DHCPEOF
DHCP_INTERFACE=${INTERFACE_LAN}
DHCP_SUBNET=${SUBNET_LAN}
DHCP_POOL_START=${DHCP_START}
DHCP_POOL_END=${DHCP_END}
DHCP_ROUTER=${ROUTER_IP}
DHCP_DNS=${DNS_SERVERS}
DHCPEOF
chmod 0600 "$MNT/var/nifty-filter/dhcp.env"

# Carry over authorized keys from the live session
echo "==> Copying SSH authorized keys..."
cp "$AUTH_KEYS" "$MNT/var/home/admin/.ssh/authorized_keys"
chmod 0700 "$MNT/var/home/admin/.ssh"
chmod 0600 "$MNT/var/home/admin/.ssh/authorized_keys"
chown -R 1000:100 "$MNT/var/home/admin"

# Preserve host keys from the live session
echo "==> Preserving SSH host keys..."
for keyfile in /var/nifty-filter/ssh/ssh_host_* /etc/ssh/ssh_host_*; do
    [[ -f "$keyfile" ]] && cp "$keyfile" "$MNT/var/nifty-filter/ssh/"
done
echo "  Host fingerprint will be preserved across systemctl reboot"

echo "==> Initializing git repo in /var/nifty-filter..."
git -C "$MNT/var/nifty-filter" init -b main
cat > "$MNT/var/nifty-filter/.gitignore" <<'GITIGNORE'
ssh/ssh_host_*
GITIGNORE
git -C "$MNT/var/nifty-filter" add -A
git -C "$MNT/var/nifty-filter" \
    -c user.name="nifty-filter" \
    -c user.email="nifty-filter@localhost" \
    commit -m "initial configuration"

if [[ -n "$GIT_REMOTE" ]]; then
    git -C "$MNT/var/nifty-filter" remote add origin "$GIT_REMOTE"
    echo "==> Cloning source repo for on-device upgrades..."
    git clone "$GIT_REMOTE" "$MNT/var/nifty-filter/src" && chown -R 1000:100 "$MNT/var/nifty-filter/src" || echo "  WARNING: Could not clone source repo. On-device upgrades will need manual setup."
    echo "  Git remote set: $GIT_REMOTE"
fi

# Make config files owned by admin
chown -R 1000:100 "$MNT/var/nifty-filter"

echo "==> Unmounting..."
umount -R "$MNT"

echo "==> Ejecting installation media..."
eject /dev/sr0 2>/dev/null || eject /dev/cdrom 2>/dev/null || true

echo ""
echo "Installation complete. Rebooting..."
systemctl reboot
