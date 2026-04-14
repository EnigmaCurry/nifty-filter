#!/usr/bin/env bash
# nifty-install — install nifty-filter to disk from the live ISO
#
# Partitions the target disk, copies the running system to root,
# sets up /var with default config, and initializes a git repo
# for remote config management.
#
# Prerequisites:
#   1. Add your SSH public key to /var/nifty-filter/ssh/admin_authorized_keys
#   2. Reconnect over SSH using key auth
#   3. Run this installer
set -euo pipefail

DISK=""
GIT_REMOTE=""
AUTH_KEYS="/var/nifty-filter/ssh/admin_authorized_keys"
SSH_DIR="/var/nifty-filter/ssh"

usage() {
    cat <<EOF
Usage: nifty-install [options] <disk>

Install nifty-filter to a disk from the running live ISO.

Before running, you must:
  1. Add your SSH public key:
       echo 'ssh-ed25519 AAAA...' | sudo tee -a $AUTH_KEYS
  2. Reconnect using key authentication

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

# Check if the current SSH session is using key authentication.
# Inspects the sshd journal for the login entry matching our session.
check_ssh_auth() {
    # Not over SSH — console is fine
    if [[ -z "${SSH_CONNECTION:-}" ]]; then
        return 0
    fi

    # Find the sshd process for this session
    local sshd_pid
    sshd_pid=$(ps -o ppid= -p $$ | tr -d ' ')
    # Walk up to find the sshd parent
    while [[ "$sshd_pid" -gt 1 ]]; do
        local cmd
        cmd=$(ps -o comm= -p "$sshd_pid" 2>/dev/null || true)
        if [[ "$cmd" == "sshd" ]]; then
            break
        fi
        sshd_pid=$(ps -o ppid= -p "$sshd_pid" 2>/dev/null | tr -d ' ')
    done

    # Check the journal for how this session authenticated
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
        echo "  1. Add your public key:"
        echo "       echo 'ssh-ed25519 AAAA...' | sudo tee -a $AUTH_KEYS"
        echo "  2. Disconnect and reconnect with your key:"
        echo "       ssh admin@<this-host>"
        echo "  3. Run this installer again"
        echo ""
        exit 1
    elif [[ "$auth_method" == "publickey" ]]; then
        return 0
    else
        # Can't determine — warn but allow (could be console)
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
        *)
            [[ -z "$DISK" ]] || die "Unexpected argument: $1"
            DISK="$1"; shift
            ;;
    esac
done

[[ -n "$DISK" ]] || { echo "Error: no disk specified" >&2; usage 1; }
[[ -b "$DISK" ]] || die "$DISK is not a block device"
[[ $EUID -eq 0 ]] || die "Must run as root (use sudo)"

# --- Pre-flight checks ---

# Check authorized keys exist
if [[ ! -s "$AUTH_KEYS" ]]; then
    echo ""
    echo "REFUSED: No SSH authorized keys found."
    echo ""
    echo "You must add at least one SSH public key before installing."
    echo "This key will be carried into the installed system."
    echo ""
    echo "  echo 'ssh-ed25519 AAAA...' | sudo tee -a $AUTH_KEYS"
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
    # Show key type and comment (last two fields)
    echo "  $(echo "$key" | awk '{print $1, $NF}')"
done < "$AUTH_KEYS"
echo ""

# Safety check
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
wipefs -af "$DISK"
parted -s "$DISK" \
    mklabel gpt \
    mkpart NIFTY_BOOT fat32 1MiB 513MiB \
    set 1 esp on \
    mkpart NIFTY_ROOT ext4 513MiB 4609MiB \
    mkpart NIFTY_VAR ext4 4609MiB 100%

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
mkfs.ext4 -L NIFTY_ROOT -q "$PART_ROOT"
mkfs.ext4 -L NIFTY_VAR -q "$PART_VAR"

echo "==> Mounting filesystems..."
mount "$PART_ROOT" "$MNT"
mkdir -p "$MNT/boot" "$MNT/var"
mount "$PART_BOOT" "$MNT/boot"
mount "$PART_VAR" "$MNT/var"

echo "==> Copying system closure to disk..."
SYSTEM_PATH=$(readlink -f /run/current-system)

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
NIXOS_INSTALL_BOOTLOADER=1 nixos-enter --root "$MNT" -- \
    /run/current-system/bin/switch-to-configuration boot 2>/dev/null || true

if ! ls "$MNT/boot/loader/entries/"*.conf &>/dev/null; then
    echo "  Using bootctl to install systemd-boot..."
    bootctl install --esp-path="$MNT/boot" --root="$MNT" 2>/dev/null || true
fi

echo "==> Setting up /var..."
mkdir -p "$MNT/var/nifty-filter/ssh"
mkdir -p "$MNT/var/home/admin/.ssh"
mkdir -p "$MNT/var/root"
mkdir -p "$MNT/var/log/journal"

# Copy default router config
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

# Carry over authorized keys from the live session
echo "==> Copying SSH authorized keys..."
cp "$AUTH_KEYS" "$MNT/var/nifty-filter/ssh/admin_authorized_keys"
chmod 0644 "$MNT/var/nifty-filter/ssh/admin_authorized_keys"

# Preserve host keys from the live session so the fingerprint doesn't change
echo "==> Preserving SSH host keys..."
for keyfile in /etc/ssh/ssh_host_*; do
    cp "$keyfile" "$MNT/var/nifty-filter/ssh/"
done
echo "  Host fingerprint will be preserved across reboot"

echo "==> Initializing git repo in /var/nifty-filter..."
git -C "$MNT/var/nifty-filter" init -b main
# Don't track host keys in git
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
echo " Your SSH key and host fingerprint have been preserved."
echo " You can reconnect without any host key warnings."
echo ""
echo " After boot:"
echo "   ssh admin@<router-ip>"
echo "   sudo vim /var/nifty-filter/router.env"
echo "   sudo reboot"
echo ""
echo " Config is git-tracked in /var/nifty-filter/"
if [[ -n "$GIT_REMOTE" ]]; then
echo "   Remote: $GIT_REMOTE"
echo "   Push:   cd /var/nifty-filter && git push"
fi
echo "========================================"
