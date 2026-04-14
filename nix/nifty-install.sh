#!/usr/bin/env bash
# nifty-install — install nifty-filter to disk from the live ISO
#
# Partitions the target disk, copies the running system to root,
# sets up /var with default config, and initializes a git repo
# for remote config management.
set -euo pipefail

DISK=""
GIT_REMOTE=""

usage() {
    cat <<EOF
Usage: nifty-install [options] <disk>

Install nifty-filter to a disk from the running live ISO.

Arguments:
  disk              Target disk (e.g. /dev/sda, /dev/vda)

Options:
  --git-remote URL  Set a git remote for config updates
  -h, --help        Show this help

Disk layout:
  Partition 1:  512M   EFI System Partition  (NIFTY_BOOT)
  Partition 2:  4G     Root (read-only)      (NIFTY_ROOT)
  Partition 3:  rest   /var (read-write)     (NIFTY_VAR)

Example:
  nifty-install /dev/vda
  nifty-install --git-remote git@github.com:user/router-config.git /dev/sda
EOF
    exit "${1:-0}"
}

die() { echo "ERROR: $*" >&2; exit 1; }

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --git-remote) GIT_REMOTE="$2"; shift 2 ;;
        -h|--help) usage 0 ;;
        -*) die "Unknown option: $1" ;;
        *)
            [[ -z "$DISK" ]] || die "Unexpected argument: $1"
            DISK="$1"; shift
            ;;
    esac
done

[[ -n "$DISK" ]] || { echo "Error: no disk specified" >&2; usage 1; }
[[ -b "$DISK" ]] || die "$DISK is not a block device"
[[ $EUID -eq 0 ]] || die "Must run as root (use sudo)"

# Safety check
echo ""
echo "WARNING: This will ERASE ALL DATA on $DISK"
echo ""
echo "  Disk: $DISK"
lsblk -no SIZE,MODEL "$DISK" 2>/dev/null | sed 's/^/  /'
echo ""
read -rp "Type YES to continue: " confirm
[[ "$confirm" == "YES" ]] || { echo "Aborted."; exit 1; }

MNT=$(mktemp -d)
trap 'umount -R "$MNT" 2>/dev/null || true; rmdir "$MNT" 2>/dev/null || true' EXIT

echo ""
echo "==> Partitioning $DISK..."
# Wipe and partition
wipefs -af "$DISK"
parted -s "$DISK" \
    mklabel gpt \
    mkpart NIFTY_BOOT fat32 1MiB 513MiB \
    set 1 esp on \
    mkpart NIFTY_ROOT ext4 513MiB 4609MiB \
    mkpart NIFTY_VAR ext4 4609MiB 100%

# Wait for partitions to appear
udevadm settle
sleep 1

# Detect partition paths (handles /dev/sda1 and /dev/vda1 and /dev/nvme0n1p1)
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
mkfs.ext4 -L NIFTY_ROOT -q "$PART_ROOT"
mkfs.ext4 -L NIFTY_VAR -q "$PART_VAR"

echo "==> Mounting filesystems..."
mount "$PART_ROOT" "$MNT"
mkdir -p "$MNT/boot" "$MNT/var"
mount "$PART_BOOT" "$MNT/boot"
mount "$PART_VAR" "$MNT/var"

echo "==> Copying system closure to disk..."
# The running system's toplevel path
SYSTEM_PATH=$(readlink -f /run/current-system)

# Copy the nix store paths needed by the system
mkdir -p "$MNT/nix/store"
# Get all store paths in the closure
nix-store -qR "$SYSTEM_PATH" | while read -r path; do
    echo "  copying $(basename "$path")"
    cp -a "$path" "$MNT/nix/store/"
done

# Copy nix database so the installed system knows what's in its store
mkdir -p "$MNT/nix/var/nix/db"
nix-store --dump-db | nix-store --load-db --store "$MNT"

echo "==> Setting up system profile..."
mkdir -p "$MNT/nix/var/nix/profiles"
ln -sfn "$SYSTEM_PATH" "$MNT/nix/var/nix/profiles/system"

echo "==> Installing bootloader..."
NIXOS_INSTALL_BOOTLOADER=1 nixos-enter --root "$MNT" -- \
    /run/current-system/bin/switch-to-configuration boot 2>/dev/null || true

# Fall back to bootctl if switch-to-configuration didn't work
if ! ls "$MNT/boot/loader/entries/"*.conf &>/dev/null; then
    echo "  Using bootctl to install systemd-boot..."
    bootctl install --esp-path="$MNT/boot" --root="$MNT" 2>/dev/null || true
fi

echo "==> Setting up /var..."
mkdir -p "$MNT/var/nifty-filter/ssh"
mkdir -p "$MNT/var/home"
mkdir -p "$MNT/var/root"
mkdir -p "$MNT/var/log/journal"

# Copy default config
cp /etc/nifty-filter/default-router.env "$MNT/var/nifty-filter/router.env" 2>/dev/null || \
    cp /run/current-system/sw/etc/nifty-filter/default-router.env "$MNT/var/nifty-filter/router.env" 2>/dev/null || \
    cat > "$MNT/var/nifty-filter/router.env" <<'ENVEOF'
INTERFACE_LAN=enp1s0
INTERFACE_WAN=enp2s0
SUBNET_LAN=192.168.10.1/24
ICMP_ACCEPT_LAN=echo-request,echo-reply,destination-unreachable,time-exceeded
ICMP_ACCEPT_WAN=
TCP_ACCEPT_LAN=22
UDP_ACCEPT_LAN=
TCP_ACCEPT_WAN=
UDP_ACCEPT_WAN=
TCP_FORWARD_LAN=
UDP_FORWARD_LAN=
TCP_FORWARD_WAN=
UDP_FORWARD_WAN=
ENVEOF
chmod 0600 "$MNT/var/nifty-filter/router.env"

# Seed empty authorized_keys
touch "$MNT/var/nifty-filter/ssh/admin_authorized_keys"
chmod 0644 "$MNT/var/nifty-filter/ssh/admin_authorized_keys"

echo "==> Initializing git repo in /var/nifty-filter..."
git -C "$MNT/var/nifty-filter" init -b main
git -C "$MNT/var/nifty-filter" add -A
git -C "$MNT/var/nifty-filter" \
    -c user.name="nifty-filter" \
    -c user.email="nifty-filter@localhost" \
    commit -m "initial configuration"

if [[ -n "$GIT_REMOTE" ]]; then
    git -C "$MNT/var/nifty-filter" remote add origin "$GIT_REMOTE"
    echo "  Git remote set: $GIT_REMOTE"
fi

echo "==> Unmounting..."
umount -R "$MNT"

echo ""
echo "========================================"
echo " Installation complete!"
echo "========================================"
echo ""
echo " Remove the installation media and reboot."
echo ""
echo " After boot:"
echo "   1. SSH in:   ssh admin@<router-ip>"
echo "   2. Edit:     sudo vim /var/nifty-filter/router.env"
echo "   3. Reboot:   sudo reboot"
echo ""
echo " Config is git-tracked in /var/nifty-filter/"
if [[ -n "$GIT_REMOTE" ]]; then
echo "   Remote: $GIT_REMOTE"
echo "   Push:   cd /var/nifty-filter && git push"
fi
echo "========================================"
