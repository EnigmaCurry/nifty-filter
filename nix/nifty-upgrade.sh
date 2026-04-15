#!/usr/bin/env bash
# nifty-upgrade — upgrade the system from the router itself
#
# Must be run in maintenance mode (sudo nifty-maintenance first).
# Pulls the latest source, builds the system, updates boot entries,
# and reboots back to normal (read-only) mode.
set -euo pipefail

REPO_DIR="/var/nifty-filter/src"
REPO_REMOTE=""

[[ $EUID -eq 0 ]] || { echo "Must run as root (use sudo)"; exit 1; }

# Check we're in maintenance mode
if ! grep -q 'nifty.maintenance=1' /proc/cmdline 2>/dev/null; then
    echo "ERROR: Not in maintenance mode."
    echo ""
    echo "Run 'sudo nifty-maintenance' first to reboot into maintenance mode,"
    echo "then run this command again."
    exit 1
fi

# Check the source repo exists
if [ ! -d "$REPO_DIR/.git" ]; then
    # Try to clone from the config repo's remote
    REPO_REMOTE=$(git -C /var/nifty-filter remote get-url origin 2>/dev/null || echo "https://github.com/EnigmaCurry/nifty-filter")
    echo "==> Cloning source repo from $REPO_REMOTE..."
    git clone "$REPO_REMOTE" "$REPO_DIR"
fi

echo "==> Pulling latest source..."
cd "$REPO_DIR"
git pull

echo "==> Building system closure..."
SYSTEM_PATH=$(nix build .#nixosConfigurations.router-x86_64.config.system.build.toplevel --print-out-paths --no-link)
echo "  System: $SYSTEM_PATH"

# Check if already current
CURRENT=$(readlink -f /nix/var/nix/profiles/system 2>/dev/null || echo "")
if [ "$CURRENT" = "$SYSTEM_PATH" ]; then
    echo ""
    echo "System is already up to date."
    echo "Rebooting into normal mode..."
    reboot
fi

echo ""
script-wizard confirm "Apply upgrade and reboot?" || { echo "Aborted."; exit 1; }
echo ""

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

echo "==> Collecting garbage..."
nix-collect-garbage -d 2>/dev/null || true

echo ""
echo "Upgrade complete. Rebooting into normal (read-only) mode..."
reboot
