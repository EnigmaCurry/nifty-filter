#!/usr/bin/env bash
# nifty-upgrade — upgrade the system in place
#
# Temporarily remounts filesystems read-write, pulls the latest source,
# builds the system, updates boot entries, and reboots into the new system.
set -euo pipefail

REPO_DIR="/var/nifty-filter/src"
REPO_REMOTE=""

[[ $EUID -eq 0 ]] || exec sudo "$0" "$@"

# Ensure filesystems are writable
echo "==> Remounting filesystems read-write..."
mount -o remount,rw /
mount -o remount,rw /nix/store

# Check the source repo exists
if [ ! -d "$REPO_DIR/.git" ]; then
    REPO_REMOTE=$(git -C /var/nifty-filter remote get-url origin 2>/dev/null || echo "https://github.com/EnigmaCurry/nifty-filter")
    echo "==> Cloning source repo from $REPO_REMOTE..."
    git clone "$REPO_REMOTE" "$REPO_DIR"
    chown -R 1000:100 "$REPO_DIR"
fi

echo "==> Pulling latest source..."
cd "$REPO_DIR"
BRANCH=$(git symbolic-ref --short HEAD 2>/dev/null || echo "")
if [ "$BRANCH" != "master" ]; then
    git fetch origin master
    git checkout master
fi
git pull

echo "==> Building system closure..."
export TMPDIR=/var/tmp
mkdir -p /var/tmp
SYSTEM_PATH=$(nix build .#nixosConfigurations.router-x86_64.config.system.build.toplevel --print-out-paths --no-link)
echo "  System: $SYSTEM_PATH"

# Check if already current
CURRENT=$(readlink -f /nix/var/nix/profiles/system 2>/dev/null || echo "")
if [ "$CURRENT" = "$SYSTEM_PATH" ]; then
    echo ""
    echo "System is already up to date."
    mount -o remount,ro /nix/store 2>/dev/null || true
    mount -o remount,ro / 2>/dev/null || true
    exit 0
fi

echo "==> Updating system profile..."
ln -sfn "$SYSTEM_PATH" /nix/var/nix/profiles/system

echo "==> Updating boot entries..."
KERNEL=$(readlink -f "$SYSTEM_PATH/kernel")
INITRD=$(readlink -f "$SYSTEM_PATH/initrd")
KERNEL_PARAMS=$(cat "$SYSTEM_PATH/kernel-params" 2>/dev/null || echo "")

cp "$KERNEL" /boot/kernel
cp "$INITRD" /boot/initrd

printf 'title   nifty-filter\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s\n' \
    "$SYSTEM_PATH" "$KERNEL_PARAMS" > /boot/loader/entries/nifty-filter.conf

printf 'title   nifty-filter (maintenance)\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s rw nifty.maintenance=1\n' \
    "$SYSTEM_PATH" "$KERNEL_PARAMS" > /boot/loader/entries/nifty-filter-maintenance.conf

echo "  Boot entries updated"

echo "==> Remounting filesystems read-only..."
mount -o remount,ro /nix/store 2>/dev/null || true
mount -o remount,ro / 2>/dev/null || true

echo ""
echo "Upgrade complete. Rebooting..."
systemctl reboot
