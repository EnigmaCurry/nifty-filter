#!/usr/bin/env bash
# nifty-maintenance — reboot into maintenance mode (read-write root)
#
# In maintenance mode:
#   - Root filesystem is mounted read-write
#   - Nix store is writable, so you can build and upgrade
#   - After maintenance, reboot to return to normal (read-only) mode
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo "Must run as root (use sudo)"; exit 1; }

echo "Setting next boot to maintenance mode..."
bootctl set-oneshot nifty-filter-maintenance.conf
echo "Rebooting into maintenance mode..."
echo ""
echo "After maintenance, just reboot to return to normal mode."
reboot
