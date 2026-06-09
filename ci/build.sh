#!/usr/bin/env bash
set -euo pipefail

NIFTY_SSH_KEYS="" nix build .#pve-image --impure

src=$(find result/ -maxdepth 1 -type f \( -name '*.qcow2' -o -name '*.raw' \) | head -1)
if [ -z "$src" ]; then
    echo "ERROR: No disk image found in result/"
    ls -la result/
    exit 1
fi
echo "Source image:"
ls -lh "$src"

date_stamp=$(date +%Y%m%d)
git_sha=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
dest="output/export/nifty-filter-${date_stamp}-${git_sha}.qcow2"
mkdir -p output/export
rm -f output/export/nifty-filter-*.qcow2
nix shell nixpkgs#qemu-utils -c qemu-img convert -f qcow2 -O qcow2 -c "$src" "$dest"
echo "Exported: $dest"
ls -lh output/export/
