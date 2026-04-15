#!/usr/bin/env bash
# nifty-maintenance — reboot into maintenance mode (read-write root)
#
# In maintenance mode:
#   - Root filesystem is mounted read-write
#   - Nix store is writable, so you can build and upgrade
#   - After maintenance, reboot to return to normal (read-only) mode
set -euo pipefail

[[ $EUID -eq 0 ]] || exec sudo "$0" "$@"

echo "This will reboot into maintenance mode (read-write root)."
echo "After maintenance, reboot to return to normal mode."
echo ""
script-wizard confirm "Reboot into maintenance mode?" || { echo "Aborted."; exit 1; }

bootctl set-oneshot nifty-filter-maintenance.conf
reboot
