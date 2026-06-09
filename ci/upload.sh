#!/usr/bin/env bash
set -euo pipefail

rclone() { nix run nixpkgs#rclone -- "$@"; }
jq() { nix run nixpkgs#jq -- "$@"; }

echo "Upload image:"
ls -lh output/export/

# Clean up stale multipart uploads from previous failed runs
rclone cleanup "remote:$S3_BUCKET/" -v || true

# Delete old images
rclone delete "remote:$S3_BUCKET/" --include "nifty-filter-*.qcow2" -v

# Upload new image
rclone copy output/export/ "remote:$S3_BUCKET/" --include '*.qcow2' -v --s3-chunk-size 64M --s3-no-check-bucket

# Update manifest
rclone copy "remote:$S3_BUCKET/manifest.json" /tmp/manifest/ 2>/dev/null || true
manifest='{"images":{}}'
if [ -f /tmp/manifest/manifest.json ]; then
    # Only preserve the "images" key; discard any foreign keys (e.g. stale "profiles")
    manifest=$(cat /tmp/manifest/manifest.json | jq '{images: (.images // {})}')
fi

# Prune stale entries
bucket_files=$(rclone lsf "remote:$S3_BUCKET/" --include 'nifty-filter-*.qcow2')
for key in $(echo "$manifest" | jq -r '.images // {} | keys[]'); do
    entry_filename=$(echo "$manifest" | jq -r --arg k "$key" '.images[$k].filename')
    if ! echo "$bucket_files" | grep -qxF "$entry_filename"; then
        manifest=$(echo "$manifest" | jq --arg k "$key" 'del(.images[$k])')
    fi
done

# Add new entry
for f in output/export/nifty-filter-*.qcow2; do
    filename=$(basename "$f")
    date_stamp=$(echo "$filename" | grep -o '[0-9]\{8\}')
    git_sha=$(echo "$filename" | sed 's/.*-\([a-f0-9]*\)\.qcow2$/\1/')
    sha256=$(sha256sum "$f" | cut -d' ' -f1)
    size=$(stat --printf='%s' "$f")
    url="${S3_PUBLIC_URL%/}/${filename}"
    manifest=$(echo "$manifest" | jq \
        --arg key "nifty-filter" \
        --arg url "$url" \
        --arg filename "$filename" \
        --arg date "$date_stamp" \
        --arg commit "$git_sha" \
        --arg sha256 "$sha256" \
        --arg size "$size" \
        '.images[$key] = {url: $url, filename: $filename, date: $date, commit: $commit, sha256: $sha256, size: ($size | tonumber)}')
done

manifest=$(echo "$manifest" | jq --arg ts "$(date +%s)" '.updated = ($ts | tonumber)')
echo "$manifest" | jq . > /tmp/manifest.json
echo "Manifest:"
cat /tmp/manifest.json
rclone copyto /tmp/manifest.json "remote:$S3_BUCKET/manifest.json" --s3-no-check-bucket -v
