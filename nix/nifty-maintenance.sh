#!/bin/sh
# nifty-maintenance — reboot into maintenance mode (read-write root)
#
# In maintenance mode:
#   - Root filesystem is mounted read-write
#   - Nix store is writable, so you can build and upgrade
#   - After maintenance, reboot to return to normal (read-only) mode
set -eu

[ "$(id -u)" -eq 0 ] || exec sudo "$0" "$@"

echo "This will reboot into maintenance mode (read-write root)."
echo "After maintenance, reboot to return to normal mode."
echo ""
printf "Reboot into maintenance mode? [y/N] "
read -r answer
case "$answer" in
    [Yy]*) ;;
    *) echo "Aborted."; exit 1 ;;
esac

bootctl set-oneshot nifty-filter-maintenance.conf
systemctl reboot
