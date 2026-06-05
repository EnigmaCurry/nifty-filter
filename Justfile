set export

current_dir := `pwd`
RUST_LOG := "debug"
RUST_BACKTRACE := "1"
GIT_REMOTE := "origin"
#RUSTFLAGS := "-D warnings"

# print help for Just targets
help:
    @just -l

# Install dependencies
deps:
    @echo
    @echo "Installing dependencies:"
    @echo
    cargo install --locked cargo-nextest
    cargo install --locked git-cliff
    cargo install --locked cargo-llvm-cov
    @echo
    @echo "All dependencies have been installed."
    @echo
    @echo 'Type `just run` to build and run the development binary, and specify any args after that.'
    @echo 'For example: `just run help`'
    @echo

# Install binary dependencies (gh-actions)
bin-deps:
    cargo binstall --no-confirm cargo-nextest
    cargo binstall --no-confirm git-cliff
    cargo binstall --no-confirm cargo-llvm-cov

# Build and run binary + args
[no-cd]
run *args:
    cargo run --manifest-path "${current_dir}/Cargo.toml" -- --verbose --env-file dev.env {{args}}

watch *args:
    cargo watch -s "cargo run --quiet --manifest-path \"${current_dir}/Cargo.toml\" -- --verbose --env-file dev.env {{args}} | tee ~/sshfs/router/nftables.nft | less"

# Build + args
build *args:
    cargo build {{args}}

# Build continuously on file change
build-watch *args:
    cargo watch -s "clear && cargo build {{args}}"

# Run tests
test *args:
    cargo nextest run {{args}}

# Run tests continuously on file change
test-watch *args:
    cargo watch -s "clear && cargo nextest run {{args}}"

# Run tests with verbose logging
test-verbose *args:
    RUST_TEST_THREADS=1 cargo nextest run --nocapture {{args}}

# Run tests continuously with verbose logging
test-watch-verbose *args:
    RUST_TEST_THREADS=1 cargo watch -s "clear && cargo nextest run --nocapture -- {{args}}"

# Build coverage report
test-coverage *args: clean
    cargo llvm-cov nextest {{args}}  && \
    cargo llvm-cov {{args}} report --html

# Continuously build coverage report and serve HTTP report
test-coverage-watch *args:
    cargo watch -s "clear && just test-coverage {{args}} && cd target/llvm-cov/html && python -m http.server"

# Run Clippy to report and fix lints
clippy *args:
    echo "Compiling ..."
    RUSTFLAGS="-D warnings" cargo clippy {{args}} --quiet --color=always 2>&1 --tests | less -R

# Run Clippy continuously on file change
clippy-watch *args:
    cargo watch -s "clear && just clippy {{args}}"
    
# Bump release version and create PR branch
bump-version:
    @if [ -n "$(git status --porcelain)" ]; then echo "## Git status is not clean. Commit your changes before bumping version."; exit 1; fi
    @if [ "$(git symbolic-ref --short HEAD)" != "master" ]; then echo "## You may only bump the version from the master branch."; exit 1; fi
    source ./funcs.sh; \
    set -eo pipefail; \
    CURRENT_VERSION=$(grep -Po '^version = \K.*' Cargo.toml | sed -e 's/"//g' | head -1); \
    VERSION=$(git cliff --bumped-version | sed 's/^v//'); \
    echo; \
    (if git rev-parse v${VERSION} 2>/dev/null; then \
      echo "New version tag already exists: v${VERSION}" && \
      echo "If you need to re-do this release, delete the existing tag (git tag -d v${VERSION})" && \
      exit 1; \
     fi \
    ); \
    echo "## Current $(grep '^version =' Cargo.toml | head -1)"; \
    confirm yes "New version would be \"v${VERSION}\"" " -- Proceed?"; \
    git checkout -B release-v${VERSION}; \
    cargo set-version ${VERSION}; \
    sed -i "s/^VERSION=v.*$/VERSION=v${VERSION}/" README.md; \
    cargo update; \
    git add Cargo.toml Cargo.lock README.md; \
    git commit -m "release: v${VERSION}"; \
    echo "Bumped version: v${VERSION}"; \
    echo "Created new branch: release-v${VERSION}"; \
    echo "You should push this branch and create a PR for it."

# Tag and release a new version from master branch
release:
    @if [ -n "$(git status --porcelain)" ]; then echo "## Git status is not clean. Commit your changes before bumping version."; exit 1; fi
    @if [ "$(git symbolic-ref --short HEAD)" != "master" ]; then echo "## You may only release the master branch."; exit 1; fi
    git remote update;
    @if [[ "$(git status -uno)" != *"Your branch is up to date"* ]]; then echo "## Git branch is not in sync with git remote ${GIT_REMOTE}."; exit 1; fi;
    @set -eo pipefail; \
    source ./funcs.sh; \
    CURRENT_VERSION=$(grep -Po '^version = \K.*' Cargo.toml | sed -e 's/"//g' | head -1); \
    if git rev-parse "v${CURRENT_VERSION}" >/dev/null 2>&1; then echo "Tag already exists: v${CURRENT_VERSION}"; exit 1; fi; \
    if (git ls-remote --tags "${GIT_REMOTE}" | grep -q "refs/tags/v${CURRENT_VERSION}" >/dev/null 2>&1); then echo "Tag already exists on remote ${GIT_REMOTE}: v${CURRENT_VERSION}"; exit 1; fi; \
    cargo audit | less; \
    confirm yes "New tag will be \"v${CURRENT_VERSION}\"" " -- Proceed?"; \
    git tag "v${CURRENT_VERSION}"; \
    git push "${GIT_REMOTE}" tag "v${CURRENT_VERSION}";

# Test SSH connection to Proxmox VE host and print instance info
pve-status pve_host:
    #!/usr/bin/env bash
    set -eo pipefail
    REMOTE="root@{{pve_host}}"
    echo "Connecting to {{pve_host}}..."
    ssh ${REMOTE} '
        echo "User: $(whoami)"
        echo "Host: $(hostname)"
        echo "PVE version: $(pveversion)"
        echo "Uptime:$(uptime)"
        echo ""
        echo "VMs:"
        qm list 2>/dev/null || echo "  (none)"
    '

# Build PVE disk image (pre-partitioned, ready to import)
pve-image pve_host="":
    #!/usr/bin/env bash
    set -eo pipefail
    KEYS="$(ssh-add -L 2>/dev/null || true)"
    if [ -z "${KEYS}" ]; then
        echo "ERROR: No keys found in SSH agent (ssh-add -L returned nothing)"
        exit 1
    fi
    export NIFTY_SSH_KEYS="${KEYS}"
    NIFTY_PVE_HOST="{{pve_host}}" \
    NIFTY_STEP_CA_ROOT_CERT="$(pwd)/certs/{{pve_host}}/step-ca-root.crt" \
        nix build .#pve-image --impure
    echo ""
    echo "PVE disk image built successfully:"
    echo "  $(ls result/)"

# Build NixOS router ISO image
iso:
    nix build .#iso --impure
    @echo ""
    @echo "ISO built successfully (branch: $(git symbolic-ref --short HEAD 2>/dev/null || echo master)):"
    @echo "  $(readlink -f result/iso/*.iso)"
    @echo ""
    @echo "Flash to USB:"
    @echo "  sudo dd if=$(readlink -f result/iso/*.iso) of=/dev/sdX bs=4M status=progress"
    @echo ""
    @echo "ISO built successfully:"
    @echo "  $(readlink -f result/iso/*.iso)"
    @echo ""
    @echo "Flash to USB:"
    @echo "  sudo dd if=$(readlink -f result/iso/*.iso) of=/dev/sdX bs=4M status=progress"

# Build NixOS router ISO with full hardware support (linux-firmware + all drivers)
iso-big:
    nix build .#iso-big --impure
    @echo ""
    @echo "ISO (big) built successfully (branch: $(git symbolic-ref --short HEAD 2>/dev/null || echo master)):"
    @echo "  $(readlink -f result/iso/*.iso)"
    @echo ""
    @echo "Flash to USB:"
    @echo "  sudo dd if=$(readlink -f result/iso/*.iso) of=/dev/sdX bs=4M status=progress"

# Upgrade a remote router (builds locally, stages for next reboot)
upgrade host:
    #!/usr/bin/env bash
    set -eo pipefail

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-upgrade-%C -o ControlPersist=60"
    REMOTE="admin@{{host}}"

    # Open persistent SSH connection (authenticates once)
    echo "Connecting to {{host}}..."
    ssh ${SSH_OPTS} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    echo "Building system closure..."
    nix build .#nixosConfigurations.router-x86_64.config.system.build.toplevel
    SYSTEM_PATH="$(readlink -f result)"
    echo "System: ${SYSTEM_PATH}"

    # Get all store paths in the closure
    echo "Computing closure..."
    PATHS=$(nix-store -qR "${SYSTEM_PATH}")
    TOTAL=$(echo "${PATHS}" | wc -l)

    # Remount root and nix store rw on the remote
    echo "Remounting as read-write on {{host}}..."
    ssh ${SSH_OPTS} ${REMOTE} 'sudo mount -o remount,rw / && sudo mount -o remount,rw /nix/store'

    # Find which paths are missing on the remote
    echo "Checking ${TOTAL} store paths..."
    MISSING=$(echo "${PATHS}" | ssh ${SSH_OPTS} ${REMOTE} 'while read p; do [ -e "$p" ] || echo "$p"; done')
    MISSING_COUNT=$(echo "${MISSING}" | grep -c . || true)

    # Check if remote is already running this exact system
    RUNNING=$(ssh ${SSH_OPTS} ${REMOTE} readlink -f /run/current-system 2>/dev/null || echo "")
    if [ "${RUNNING}" = "${SYSTEM_PATH}" ] && [ "${MISSING_COUNT}" -eq 0 ]; then
        echo ""
        echo "{{host}} is already up to date."
        exit 0
    fi

    if [ "${MISSING_COUNT}" -gt 0 ]; then
        echo "Copying ${MISSING_COUNT} store paths to {{host}}..."
        for path in ${MISSING}; do
            echo "  $(basename ${path})"
            if [ -d "${path}" ]; then
                rsync -a -e "ssh ${SSH_OPTS}" --rsync-path="sudo rsync" "${path}/" "${REMOTE}:${path}/"
            else
                rsync -a -e "ssh ${SSH_OPTS}" --rsync-path="sudo rsync" "${path}" "${REMOTE}:${path}"
            fi
        done
    fi

    echo "Setting boot profile and bootloader on {{host}}..."
    ssh ${SSH_OPTS} ${REMOTE} bash -s -- "${SYSTEM_PATH}" <<'REMOTE_SCRIPT'
    set -eo pipefail
    SYSTEM_PATH="$1"
    sudo ln -sfn "${SYSTEM_PATH}" /nix/var/nix/profiles/system
    KERNEL=$(readlink -f "${SYSTEM_PATH}/kernel")
    INITRD=$(readlink -f "${SYSTEM_PATH}/initrd")
    KERNEL_PARAMS=$(cat "${SYSTEM_PATH}/kernel-params" 2>/dev/null || echo "")
    sudo cp "${KERNEL}" /boot/kernel
    sudo cp "${INITRD}" /boot/initrd
    printf 'title   nifty-filter\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s\n' "${SYSTEM_PATH}" "${KERNEL_PARAMS}" | sudo tee /boot/loader/entries/nifty-filter.conf > /dev/null
    printf 'title   nifty-filter (maintenance)\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s rw nifty.maintenance=1\n' "${SYSTEM_PATH}" "${KERNEL_PARAMS}" | sudo tee /boot/loader/entries/nifty-filter-maintenance.conf > /dev/null
    printf 'timeout 5\ndefault nifty-filter.conf\nconsole-mode keep\n' | sudo tee /boot/loader/loader.conf > /dev/null
    sudo mount -o remount,ro /nix/store
    sudo mount -o remount,ro /
    nohup sudo reboot &>/dev/null &
    REMOTE_SCRIPT
    echo ""
    echo "Upgrade applied. {{host}} is rebooting..."

# Interactive upgrade menu for PVE environment
pve-upgrade-menu pve_host:
    #!/usr/bin/env bash
    set -eo pipefail
    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    CHOICE=$(script-wizard choose "What to upgrade?" \
        "Step-CA VM" \
        "Router VM (full rebuild + reboot)" \
        "Services VM (including technitium)" \
        "Dashboard only (no reboot)")
    case "${CHOICE}" in
        "Step-CA VM"*)
            echo "Pulling latest nifty-filter..."
            git pull
            echo "Pulling latest nixos-vm-template..."
            git -C "${VM_TEMPLATE_DIR}" pull
            echo "Updating nifty-filter flake input..."
            nix flake update nifty-filter --flake "${VM_TEMPLATE_DIR}"
            just pve-upgrade-step-ca "{{pve_host}}"
            ;;
        "Router VM"*)
            echo "Pulling latest nifty-filter..."
            git pull
            just pve-upgrade "{{pve_host}}" 101 nifty-filter
            ;;
        "Services VM"*)
            echo "Pulling latest nifty-filter..."
            git pull
            echo "Pulling latest nixos-vm-template..."
            git -C "${VM_TEMPLATE_DIR}" pull
            echo "Updating nifty-filter flake input..."
            nix flake update nifty-filter --flake "${VM_TEMPLATE_DIR}"
            just pve-upgrade-services "{{pve_host}}"
            ;;
        "Dashboard only"*)
            echo "Pulling latest nifty-filter..."
            git pull
            just pve-deploy-dashboard "{{pve_host}}"
            ;;
    esac

# Upgrade a remote router VM via PVE jump host (builds locally, stages for next reboot)
pve-upgrade pve_host vmid="101" vm_name="nifty-filter" target_ip="10.99.0.1":
    #!/usr/bin/env bash
    set -eo pipefail

    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    PVE_REMOTE="root@${PVE_HOST}"
    REMOTE="admin@{{target_ip}}"
    PROXY="-J ${PVE_REMOTE}"

    # Verify VM name matches VMID on PVE host
    ACTUAL_NAME=$(ssh ${PVE_REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-pve-upgrade-%C -o ControlPersist=600 -o ServerAliveInterval=30"

    # Open persistent SSH connection through PVE jump host (authenticates once)
    echo "Connecting to {{target_ip}} via ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    echo "Building system closure..."
    NIFTY_PVE_HOST="${PVE_HOST}" \
    NIFTY_STEP_CA_ROOT_CERT="$(pwd)/certs/${PVE_HOST}/step-ca-root.crt" \
    nix build .#nixosConfigurations.pve-router-x86_64.config.system.build.toplevel --impure
    SYSTEM_PATH="$(readlink -f result)"
    echo "System: ${SYSTEM_PATH}"

    # Get all store paths in the closure
    echo "Computing closure..."
    PATHS=$(nix-store -qR "${SYSTEM_PATH}")
    TOTAL=$(echo "${PATHS}" | wc -l)

    # Remount root and nix store rw on the remote
    echo "Remounting as read-write on {{target_ip}}..."
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} 'sudo mount -o remount,rw / && sudo mount -o remount,rw /nix/store'

    # Find which paths are missing on the remote
    echo "Checking ${TOTAL} store paths..."
    MISSING=$(echo "${PATHS}" | ssh ${SSH_OPTS} ${PROXY} ${REMOTE} 'while read p; do [ -e "$p" ] || echo "$p"; done')
    MISSING_COUNT=$(echo "${MISSING}" | grep -c . || true)

    # Check if remote is already running this exact system
    RUNNING=$(ssh ${SSH_OPTS} ${PROXY} ${REMOTE} readlink -f /run/current-system 2>/dev/null || echo "")
    if [ "${RUNNING}" = "${SYSTEM_PATH}" ] && [ "${MISSING_COUNT}" -eq 0 ]; then
        echo ""
        echo "{{vm_name}} ({{vmid}}) is already up to date."
        exit 0
    fi

    # Create PVE snapshot before modifying anything
    CURRENT_VER=$(ssh ${SSH_OPTS} ${PROXY} ${REMOTE} 'nifty-filter version 2>/dev/null' || echo "unknown")
    TARGET_SHA=$(git rev-parse --short HEAD)
    SNAP_NAME="pre-upgrade-$(date +%Y%m%d-%H%M%S)"
    SNAP_DESC="Upgrade from ${CURRENT_VER} to ${TARGET_SHA}"
    echo "Creating snapshot ${SNAP_NAME} on ${PVE_HOST}..."
    echo "  ${SNAP_DESC}"
    ssh ${PVE_REMOTE} "qm snapshot ${VMID} ${SNAP_NAME} --description '${SNAP_DESC}'"
    echo "  Rollback: ssh ${PVE_REMOTE} qm rollback ${VMID} ${SNAP_NAME}"

    if [ "${MISSING_COUNT}" -gt 0 ]; then
        echo "Copying ${MISSING_COUNT} store paths to {{target_ip}}..."
        for path in ${MISSING}; do
            echo "  $(basename ${path})"
            if [ -d "${path}" ]; then
                rsync -a -e "ssh ${SSH_OPTS} ${PROXY}" --rsync-path="sudo rsync" "${path}/" "${REMOTE}:${path}/"
            else
                rsync -a -e "ssh ${SSH_OPTS} ${PROXY}" --rsync-path="sudo rsync" "${path}" "${REMOTE}:${path}"
            fi
        done
    fi

    echo "Setting boot profile and bootloader on {{target_ip}}..."
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} bash -s -- "${SYSTEM_PATH}" <<'REMOTE_SCRIPT'
    set -eo pipefail
    SYSTEM_PATH="$1"
    sudo ln -sfn "${SYSTEM_PATH}" /nix/var/nix/profiles/system
    KERNEL=$(readlink -f "${SYSTEM_PATH}/kernel")
    INITRD=$(readlink -f "${SYSTEM_PATH}/initrd")
    KERNEL_PARAMS=$(cat "${SYSTEM_PATH}/kernel-params" 2>/dev/null || echo "")
    sudo cp "${KERNEL}" /boot/kernel
    sudo cp "${INITRD}" /boot/initrd
    printf 'title   nifty-filter\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s\n' "${SYSTEM_PATH}" "${KERNEL_PARAMS}" | sudo tee /boot/loader/entries/nifty-filter.conf > /dev/null
    printf 'title   nifty-filter (maintenance)\nlinux   /kernel\ninitrd  /initrd\noptions init=%s/init %s rw nifty.maintenance=1\n' "${SYSTEM_PATH}" "${KERNEL_PARAMS}" | sudo tee /boot/loader/entries/nifty-filter-maintenance.conf > /dev/null
    printf 'timeout 5\ndefault nifty-filter.conf\nconsole-mode keep\n' | sudo tee /boot/loader/loader.conf > /dev/null
    sudo mount -o remount,ro /nix/store
    sudo mount -o remount,ro /
    nohup sudo reboot &>/dev/null &
    REMOTE_SCRIPT
    echo ""
    echo "Upgrade applied. {{vm_name}} ({{vmid}}) is rebooting..."

# Fast deploy: build nifty-dashboard with cargo and hot-swap binary on remote VM (no reboot)
pve-deploy-dashboard pve_host target_ip="10.99.0.1":
    #!/usr/bin/env bash
    set -eo pipefail
    REMOTE="admin@{{target_ip}}"
    PROXY="-J root@{{pve_host}}"
    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-deploy-dashboard-%C -o ControlPersist=60 -o ServerAliveInterval=15"
    DASHBOARD_DIR="{{current_dir}}/crates/nifty-dashboard"
    BINARY="${DASHBOARD_DIR}/target/release/nifty-dashboard"

    # Rebuild frontend if source is newer than build
    FRONTEND_DIR="${DASHBOARD_DIR}/frontend"
    NEEDS_BUILD=false
    if [ ! -d "${FRONTEND_DIR}/build" ]; then
        NEEDS_BUILD=true
    else
        NEWEST_SRC=$(find "${FRONTEND_DIR}/src" "${FRONTEND_DIR}/static" -type f -newer "${FRONTEND_DIR}/build" 2>/dev/null -print -quit)
        if [ -n "${NEWEST_SRC}" ]; then
            NEEDS_BUILD=true
        fi
    fi
    if [ "${NEEDS_BUILD}" = true ]; then
        echo "Building frontend..."
        (cd "${FRONTEND_DIR}" && pnpm install --frozen-lockfile && pnpm build)
    fi

    # Build inside nix develop so the binary links against nix store glibc (matches pve-upgrade builds)
    echo "Building nifty-dashboard (cargo release via nix develop)..."
    nix develop --command cargo build --release --manifest-path "${DASHBOARD_DIR}/Cargo.toml" -p nifty-dashboard

    # Find where the current binary lives on the remote
    echo "Connecting to {{target_ip}} via {{pve_host}}..."
    ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    REMOTE_BIN=$(ssh ${SSH_OPTS} ${PROXY} ${REMOTE} 'readlink -f $(which nifty-dashboard)')
    echo "Remote binary: ${REMOTE_BIN}"

    # Remount read-write, copy binary, restart service
    echo "Deploying binary..."
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} 'sudo mount -o remount,rw / && sudo mount -o remount,rw /nix/store'
    scp ${SSH_OPTS} -o "ProxyJump=root@{{pve_host}}" "${BINARY}" "${REMOTE}:/tmp/nifty-dashboard"
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} bash -s -- "${REMOTE_BIN}" <<'DEPLOY_SCRIPT'
    set -eo pipefail
    REMOTE_BIN="$1"
    sudo systemctl stop nifty-dashboard
    sudo cp /tmp/nifty-dashboard "${REMOTE_BIN}"
    rm /tmp/nifty-dashboard
    sudo mount -o remount,ro /nix/store
    sudo mount -o remount,ro /
    sudo systemctl start nifty-dashboard
    DEPLOY_SCRIPT
    echo ""
    echo "Dashboard deployed and restarted on {{target_ip}}."

# Create a NixOS router VM on Proxmox VE (interactive)
# A dedicated 'mgmt' bridge is always created for out-of-band management.
# Builds a pre-partitioned disk image (no ISO installer needed).
pve-install pve_host:
    #!/usr/bin/env bash
    set -eo pipefail
    source ./funcs.sh

    PVE_HOST="{{pve_host}}"
    MGMT_SUBNET="${MGMT_SUBNET:-10.99.0.0/24}"

    # --- Interactive setup via Rust binary ---
    echo "Building setup wizard..."
    nix develop --command cargo build --quiet --features nixos 2>/dev/null
    SETUP_OUTPUT=$(nix develop --command cargo run --quiet --features nixos -- pve-setup "${PVE_HOST}")
    eval "${SETUP_OUTPUT}"

    REMOTE="root@${PVE_HOST}"
    PVE_UPLOAD_DIR="${PVE_UPLOAD_DIR:-/tmp}"

    # --- Reconnect to PVE for install operations ---
    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-pve-%C -o ControlPersist=60"
    echo "Connecting to ${PVE_HOST}..."
    ssh ${SSH_OPTS} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    # --- Check available space on PVE upload directory ---
    AVAIL_KB=$(ssh ${SSH_OPTS} ${REMOTE} "df --output=avail '${PVE_UPLOAD_DIR}' 2>/dev/null | tail -1" | tr -d ' ')
    REQUIRED_KB=$((16 * 1024 * 1024))  # 16 GB in KB
    if [ -n "${AVAIL_KB}" ] && [ "${AVAIL_KB}" -lt "${REQUIRED_KB}" ]; then
        AVAIL_GB=$(( AVAIL_KB / 1024 / 1024 ))
        echo "ERROR: Only ${AVAIL_GB}G available in ${PVE_UPLOAD_DIR} on ${PVE_HOST} (need 16G)"
        echo "Set PVE_UPLOAD_DIR to use a different path, e.g.:"
        echo "  PVE_UPLOAD_DIR=/var/tmp just pve-install ${PVE_HOST}"
        exit 1
    fi

    # --- Parse management subnet ---
    MGMT_PREFIX="${MGMT_SUBNET#*/}"
    MGMT_NET="${MGMT_SUBNET%/*}"
    MGMT_BASE="${MGMT_NET%.*}"
    PVE_MGMT_IP="${MGMT_BASE}.2/${MGMT_PREFIX}"
    ROUTER_MGMT_IP="${MGMT_BASE}.1"

    # --- Classify NICs as bridge (vmbr*) or PCI ---
    PCI_DEVICES=()
    BRIDGES=()
    for nic in "${NICS[@]}"; do
        if [[ "${nic}" == vmbr* ]]; then
            BRIDGES+=("${nic}")
        else
            PCI_DEVICES+=("${nic#0000:}")
        fi
    done

    # NIFTY_SSH_KEYS is set by pve-setup (selected interactively)
    export NIFTY_SSH_KEYS

    # --- Build PVE disk image ---
    echo "Building PVE disk image..."
    nix flake update
    NIFTY_PVE_HOST="${PVE_HOST}" \
    NIFTY_STEP_CA_ROOT_CERT="$(pwd)/certs/${PVE_HOST}/step-ca-root.crt" \
        nix build .#pve-image --impure
    IMAGE_PATH="$(find result/ -maxdepth 1 -type f \( -name '*.raw' -o -name '*.img' \) | head -1)"
    if [ -z "${IMAGE_PATH}" ]; then
        echo "ERROR: No disk image found in result/. Contents:"
        ls -la result/
        exit 1
    fi
    IMAGE_PATH="$(readlink -f "${IMAGE_PATH}")"
    echo "  Image: ${IMAGE_PATH}"

    # --- Upload disk image ---
    UPLOAD_PATH="${PVE_UPLOAD_DIR}/nifty-filter-pve.raw"
    echo "Uploading disk image to ${PVE_HOST}:${UPLOAD_PATH} ..."
    rsync -ah --progress -e "ssh ${SSH_OPTS}" \
        "${IMAGE_PATH}" \
        "${REMOTE}:${UPLOAD_PATH}"
    echo "Image uploaded."

    # --- Create mgmt bridge (always, with static IP) and any user-specified bridges ---
    if ! ssh ${SSH_OPTS} ${REMOTE} "ip link show mgmt" &>/dev/null; then
        echo "Creating mgmt bridge with ${PVE_MGMT_IP} on ${PVE_HOST}..."
        ssh ${SSH_OPTS} ${REMOTE} "printf '\nauto mgmt\niface mgmt inet static\n    address %s\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0\n' '${PVE_MGMT_IP}' >> /etc/network/interfaces && ifup mgmt"
        echo "  mgmt created."
    fi
    for bridge in "${BRIDGES[@]}"; do
        if ! ssh ${SSH_OPTS} ${REMOTE} "ip link show ${bridge}" &>/dev/null; then
            echo "Creating isolated bridge ${bridge} on ${PVE_HOST}..."
            ssh ${SSH_OPTS} ${REMOTE} "printf '\nauto %s\niface %s inet manual\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0\n' '${bridge}' '${bridge}' >> /etc/network/interfaces && ifup ${bridge}"
            echo "  ${bridge} created."
        fi
    done

    # --- Discover infra bridge (created by pve-install-step-ca) ---
    INFRA_BRIDGE=""
    for candidate in vmbr2; do
        if ssh ${SSH_OPTS} ${REMOTE} "ip link show ${candidate}" &>/dev/null; then
            INFRA_BRIDGE="${candidate}"
            echo "Found infra bridge: ${INFRA_BRIDGE}"
            break
        fi
    done
    if [ -z "${INFRA_BRIDGE}" ]; then
        echo "ERROR: No infra bridge found (expected vmbr2)."
        echo "Deploy the Step-CA VM first: just pve-install-step-ca ${PVE_HOST} <step-ca-ip>"
        exit 1
    fi

    # --- Build NIC flags (mgmt is always net0, infra is last) ---
    NIC_ARGS="--net0 virtio,bridge=mgmt"
    NET_INDEX=1
    for bridge in "${BRIDGES[@]}"; do
        NIC_ARGS="${NIC_ARGS} --net${NET_INDEX} virtio,bridge=${bridge}"
        NET_INDEX=$((NET_INDEX + 1))
    done
    # Infra NIC for Step-CA / services communication
    NIC_ARGS="${NIC_ARGS} --net${NET_INDEX} virtio,bridge=${INFRA_BRIDGE}"
    INFRA_NET_INDEX=${NET_INDEX}
    NET_INDEX=$((NET_INDEX + 1))

    HOSTPCI_ARGS=""
    PCI_INDEX=0
    for dev in "${PCI_DEVICES[@]}"; do
        HOSTPCI_ARGS="${HOSTPCI_ARGS} --hostpci${PCI_INDEX} 0000:${dev},pcie=1"
        PCI_INDEX=$((PCI_INDEX + 1))
    done

    MACHINE="q35"

    # --- Create VM (no ISO, no EFI disk — bootloader is on the disk image) ---
    echo "Creating VM ${VMID} (${VM_NAME}) on ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${REMOTE} "qm create ${VMID} \
        --name ${VM_NAME} \
        --machine ${MACHINE} \
        --bios ovmf \
        --cpu host \
        --cores 2 \
        --memory 2048 \
        --efidisk0 local-lvm:1,efitype=4m,pre-enrolled-keys=0 \
        --scsihw virtio-scsi-single \
        --ostype l26 \
        --onboot 1 \
        --serial0 socket \
        --vga serial0 \
        ${NIC_ARGS} ${HOSTPCI_ARGS}"
    echo "VM ${VMID} created."

    # --- Import boot+root disk as scsi0 ---
    echo "Importing boot+root disk as scsi0..."
    ssh ${SSH_OPTS} ${REMOTE} "qm importdisk ${VMID} ${UPLOAD_PATH} local-lvm"
    ssh ${SSH_OPTS} ${REMOTE} "qm set ${VMID} --scsi0 local-lvm:vm-${VMID}-disk-1 --boot order=scsi0"
    ssh ${SSH_OPTS} ${REMOTE} "rm -f ${UPLOAD_PATH}"

    # --- Create and format /var disk (scsi1) ---
    VAR_SIZE="${VAR_SIZE:-8}"  # GiB
    echo "Creating ${VAR_SIZE}G /var disk (scsi1)..."
    ssh ${SSH_OPTS} ${REMOTE} "qm set ${VMID} --scsi1 local-lvm:${VAR_SIZE}"
    # Format /var disk as ext4 before first boot
    VAR_VOLID=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${VMID}" | grep '^scsi1:' | awk '{print $2}' | cut -d',' -f1)
    VAR_PATH=$(ssh ${SSH_OPTS} ${REMOTE} "pvesm path ${VAR_VOLID}")
    echo "Formatting /var disk (${VAR_VOLID})..."
    ssh ${SSH_OPTS} ${REMOTE} "mkfs.ext4 -F -L NIFTY_VAR -q ${VAR_PATH}"
    echo "Disks ready."

    # --- Query mgmt MAC and pass NIC role order via fw_cfg ---
    QM_CONFIG=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${VMID}")
    MGMT_MAC=$(echo "${QM_CONFIG}" | grep '^net0:' | grep -oP 'virtio=\K[^,]+')
    echo "  mgmt MAC: ${MGMT_MAC}"

    # For virtual NICs, we can read their MACs from qm config
    # For PCI passthrough, we pass the role order instead (QEMU assigns
    # PCI slots sequentially, so the VM can sort by bus address)
    NIC_ROLES="wan:trunk"
    if [ "${#NICS[@]}" -gt 2 ]; then
        EXTRA_COUNT=$(( ${#NICS[@]} - 2 ))
        for i in $(seq 1 ${EXTRA_COUNT}); do
            NIC_ROLES="${NIC_ROLES}:extra${i}"
        done
    fi
    echo "  NIC roles: ${NIC_ROLES}"

    # Build fw_cfg args: mgmt MAC + NIC role order
    FW_CFG_ARGS="-fw_cfg name=opt/nifty/mgmt_mac,string=${MGMT_MAC}"
    FW_CFG_ARGS="${FW_CFG_ARGS} -fw_cfg name=opt/nifty/nic_roles,string=${NIC_ROLES}"

    # Also pass virtual NIC MACs when we can read them
    VIRTIO_INDEX=1
    for i in "${!NICS[@]}"; do
        if [[ "${NICS[$i]}" == vmbr* ]]; then
            VNIC_MAC=$(echo "${QM_CONFIG}" | grep "^net${VIRTIO_INDEX}:" | grep -oP 'virtio=\K[^,]+')
            VIRTIO_INDEX=$((VIRTIO_INDEX + 1))
            ROLE=$(echo "${NIC_ROLES}" | cut -d, -f$((i+1)))
            if [ -n "${VNIC_MAC}" ] && [ -n "${ROLE}" ]; then
                FW_CFG_ARGS="${FW_CFG_ARGS} -fw_cfg name=opt/nifty/${ROLE}_mac,string=${VNIC_MAC}"
                echo "  ${ROLE} MAC: ${VNIC_MAC}"
            fi
        fi
    done

    # Pass infra NIC MAC so the router can identify it
    INFRA_MAC=$(echo "${QM_CONFIG}" | grep "^net${INFRA_NET_INDEX}:" | grep -oP 'virtio=\K[^,]+')
    if [ -n "${INFRA_MAC}" ]; then
        FW_CFG_ARGS="${FW_CFG_ARGS} -fw_cfg name=opt/nifty/infra_mac,string=${INFRA_MAC}"
        echo "  infra MAC: ${INFRA_MAC} (bridge: ${INFRA_BRIDGE})"
    fi

    ssh ${SSH_OPTS} ${REMOTE} "qm set ${VMID} --args '${FW_CFG_ARGS}'"

    # --- Start VM ---
    echo ""
    echo "Starting VM ${VMID}..."
    ssh ${SSH_OPTS} ${REMOTE} "qm start ${VMID}"
    echo "VM ${VMID} started."
    echo ""
    echo "PVE install complete:"
    echo "  VMID:    ${VMID}"
    echo "  Name:    ${VM_NAME}"
    echo "  Host:    ${PVE_HOST}"
    echo "  Mgmt:    ${MGMT_MAC} -> ${PVE_MGMT_IP} on mgmt bridge (PVE host)"
    echo "  WAN:     ${WAN_MAC:-unknown} (DHCP client)"
    for bridge in "${BRIDGES[@]}"; do
        echo "  Bridge:  ${bridge}"
    done
    echo "  Infra:   ${INFRA_MAC:-unknown} on ${INFRA_BRIDGE}"
    for dev in "${PCI_DEVICES[@]}"; do
        echo "  PCI:     0000:${dev}"
    done
    echo ""
    echo "SSH keys are pre-installed. Connect directly:"
    echo "  just pve-ssh ${PVE_HOST} ${ROUTER_MGMT_IP}"
    echo ""
    echo "Then run the configuration wizard (inside the VM):"
    echo "  nifty-config"

# Copy SSH key to a VM via PVE jump host (password: nifty)
pve-ssh-copy-id pve_host target_ip user="admin":
    #!/usr/bin/env bash
    set -eo pipefail
    HOSTNAME=$(hostname)
    KEYS=$(ssh-add -L | while IFS= read -r line; do
        # Append workstation hostname as comment if not already present
        if echo "$line" | grep -q "$HOSTNAME"; then
            echo "$line"
        else
            echo "$line $HOSTNAME"
        fi
    done)
    if [ -z "$KEYS" ]; then
        echo "ERROR: No keys found in SSH agent (ssh-add -L returned nothing)"
        exit 1
    fi
    ASKPASS=$(mktemp)
    trap 'rm -f "$ASKPASS"' EXIT
    printf '#!/bin/sh\necho nifty\n' > "$ASKPASS"
    chmod +x "$ASKPASS"
    echo "$KEYS" | SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force \
        ssh -o ProxyJump=root@{{pve_host}} -o StrictHostKeyChecking=accept-new \
        {{user}}@{{target_ip}} 'mkdir -p ~/.ssh && chmod 700 ~/.ssh && while IFS= read -r key; do grep -qxF "$key" ~/.ssh/authorized_keys 2>/dev/null || echo "$key" >> ~/.ssh/authorized_keys; done && chmod 600 ~/.ssh/authorized_keys'
    echo "Key(s) installed to {{user}}@{{target_ip}}"

# SSH to a VM via PVE jump host
pve-ssh pve_host target_ip user="admin":
    ssh -J root@{{pve_host}} {{user}}@{{target_ip}}

# Eject ISO and set disk boot on a Proxmox VM
pve-eject-iso pve_host vmid vm_name:
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi
    echo "Ejecting ISO and setting boot to disk on VM ${VMID} (${VM_NAME})..."
    ssh ${REMOTE} "qm set ${VMID} --delete ide2 --boot order=scsi0"
    echo "Done. Start the VM:"
    echo "  just pve-start ${PVE_HOST} ${VMID} ${VM_NAME}"

# Start a Proxmox VM after verifying its name matches
pve-start pve_host vmid vm_name:
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi
    STATUS=$(ssh ${REMOTE} "qm status ${VMID}" | awk '{print $2}')
    if [[ "${STATUS}" == "running" ]]; then
        echo "VM ${VMID} (${VM_NAME}) is already running."
    else
        echo "Starting VM ${VMID} (${VM_NAME}) on ${PVE_HOST}..."
        ssh ${REMOTE} "qm start ${VMID}"
        echo "VM ${VMID} (${VM_NAME}) started."
    fi

# Stop a Proxmox VM gracefully after verifying its name matches
pve-stop pve_host vmid vm_name:
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi
    STATUS=$(ssh ${REMOTE} "qm status ${VMID}" | awk '{print $2}')
    if [[ "${STATUS}" == "stopped" ]]; then
        echo "VM ${VMID} (${VM_NAME}) is already stopped."
    else
        echo "Shutting down VM ${VMID} (${VM_NAME}) on ${PVE_HOST}..."
        ssh ${REMOTE} "qm shutdown ${VMID}"
        echo "VM ${VMID} (${VM_NAME}) shutdown initiated."
    fi

# Destroy a Proxmox VM after verifying its name matches
pve-destroy pve_host vmid vm_name:
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi
    echo "Destroying VM ${VMID} (${VM_NAME}) on ${PVE_HOST}..."
    ssh ${REMOTE} "qm stop ${VMID} --skiplock 2>/dev/null || true; qm destroy ${VMID} --purge"
    echo "VM ${VMID} (${VM_NAME}) destroyed."

# Open SSH tunnel to managed switch admin UI (192.168.2.1) via router VM
pve-manage-switch pve_host target_ip="10.99.0.1" switch_ip="192.168.2.1" local_port="8080":
    #!/usr/bin/env bash
    set -eo pipefail
    REMOTE="admin@{{target_ip}}"
    PROXY="-J root@{{pve_host}}"
    SWITCH_IP="{{switch_ip}}"
    ROUTER_IP="${SWITCH_IP%.*}.2"
    ROUTER_CIDR="${ROUTER_IP}/24"

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-switch-%C -o ControlPersist=60"

    echo "Connecting to {{target_ip}} via {{pve_host}}..."
    ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    echo "Adding ${ROUTER_CIDR} to trunk on router..."
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} "sudo ip addr add ${ROUTER_CIDR} dev trunk 2>/dev/null || true"

    URL="http://localhost:{{local_port}}"
    echo ""
    echo "Switch admin UI available at: ${URL}"
    echo "Press Ctrl-C to close the tunnel."
    echo ""
    if command -v xdg-open &>/dev/null; then
        (sleep 2 && xdg-open "${URL}") &
    fi
    ssh ${SSH_OPTS} ${PROXY} -L {{local_port}}:${SWITCH_IP}:80 -N ${REMOTE} || true

    echo "Removing ${ROUTER_CIDR} from trunk..."
    ssh ${SSH_OPTS} ${PROXY} ${REMOTE} "sudo ip addr del ${ROUTER_CIDR} dev trunk 2>/dev/null || true"
    echo "Done."

# Open SSH tunnel to nifty-dashboard web UI on the router VM (reconnects on disconnect)
pve-manage-dashboard pve_host target_ip="10.99.0.1" dashboard_port="443" local_port="3000":
    #!/usr/bin/env bash
    set -eo pipefail
    REMOTE="admin@{{target_ip}}"
    PROXY="-J root@{{pve_host}}"
    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-dashboard-%C -o ControlPersist=3600 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o ConnectTimeout=10"
    DASHBOARD_IP=""
    OPENED_BROWSER=false

    cleanup() {
        ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true
    }
    trap cleanup EXIT

    discover_ip() {
        DASHBOARD_IP=$(ssh ${SSH_OPTS} ${PROXY} ${REMOTE} \
            "nifty-filter get -c /var/nifty-filter/nifty-filter.hcl mgmt-subnet 2>/dev/null | cut -d/ -f1" 2>/dev/null)
    }

    connect() {
        # Kill stale control socket if any
        ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true
        ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE} 2>/dev/null || return 1

        if [ -z "${DASHBOARD_IP}" ]; then
            discover_ip || return 1
            if [ -z "${DASHBOARD_IP}" ]; then
                return 1
            fi
        fi

        URL="https://localhost:{{local_port}}"
        echo "Dashboard available at: ${URL}"
        if [ "${OPENED_BROWSER}" = false ] && command -v xdg-open &>/dev/null; then
            (sleep 2 && xdg-open "${URL}") &
            OPENED_BROWSER=true
        fi

        ssh ${SSH_OPTS} ${PROXY} -L {{local_port}}:${DASHBOARD_IP}:{{dashboard_port}} -N ${REMOTE} 2>/dev/null
    }

    echo "Connecting to {{target_ip}} via {{pve_host}}..."
    echo "Press Ctrl-C to stop."
    echo ""
    while true; do
        if connect; then
            echo "Tunnel disconnected. Reconnecting in 5s..."
        else
            echo "Connection failed. Retrying in 5s..."
        fi
        sleep 5
    done

# Open SSH tunnel to Technitium DNS web admin UI on the services VM
pve-manage-technitium pve_host services_ip="10.99.2.2" router_ip="10.99.0.1" web_port="5380" local_port="5380":
    #!/usr/bin/env bash
    set -eo pipefail
    REMOTE="admin@{{router_ip}}"
    PROXY="-J root@{{pve_host}}"
    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-technitium-%C -o ControlPersist=60"

    echo "Connecting to {{router_ip}} via {{pve_host}}..."
    ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    URL="http://localhost:{{local_port}}"
    echo ""
    echo "Technitium DNS admin UI available at: ${URL}"
    echo "Press Ctrl-C to close the tunnel."
    echo ""
    if command -v xdg-open &>/dev/null; then
        (sleep 2 && xdg-open "${URL}") &
    fi
    ssh ${SSH_OPTS} ${PROXY} -L {{local_port}}:{{services_ip}}:{{web_port}} -N ${REMOTE} || true
    echo "Done."

# Create a named snapshot of a Proxmox VM
pve-snapshot pve_host vmid vm_name comment="":
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi
    VERSION=$(grep -Po '^version = \K.*' Cargo.toml | sed 's/"//g' | head -1)
    GIT_SHA=$(git rev-parse --short HEAD)
    SNAP_NAME="v${VERSION//./-}-${GIT_SHA}-$(date +%Y%m%d-%H%M%S)"
    COMMENT="{{comment}}"
    DESC="nifty-filter v${VERSION} (${GIT_SHA})${COMMENT:+ - ${COMMENT}}"
    echo "Creating snapshot ${SNAP_NAME} on ${PVE_HOST}..."
    ssh ${REMOTE} "qm snapshot ${VMID} '${SNAP_NAME}' --description '${DESC}'"
    echo "  Snapshot: ${SNAP_NAME}"
    echo "  Description: ${DESC}"
    echo "  Rollback: ssh ${REMOTE} qm rollback ${VMID} ${SNAP_NAME}"

# Delete all snapshots for a VM and create a fresh one
pve-snapshot-prune pve_host vmid vm_name:
    #!/usr/bin/env bash
    set -eo pipefail
    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{vm_name}}"
    REMOTE="root@${PVE_HOST}"
    ACTUAL_NAME=$(ssh ${REMOTE} "qm config ${VMID} 2>/dev/null | grep '^name:' | awk '{print \$2}'" || true)
    if [[ -z "${ACTUAL_NAME}" ]]; then
        echo "ERROR: VM ${VMID} does not exist on ${PVE_HOST}"
        exit 1
    fi
    if [[ "${ACTUAL_NAME}" != "${VM_NAME}" ]]; then
        echo "ERROR: VM ${VMID} is named '${ACTUAL_NAME}', not '${VM_NAME}'"
        exit 1
    fi

    # List existing snapshots (skip 'current' which is the live state)
    SNAPSHOTS=$(ssh ${REMOTE} "qm listsnapshot ${VMID}" | grep -v '^\s*`->.*current' | awk '{print $2}' | grep -v '^$' || true)
    SNAP_COUNT=$(echo "${SNAPSHOTS}" | grep -c . || true)

    if [[ "${SNAP_COUNT}" -eq 0 ]]; then
        echo "No snapshots found for VM ${VMID} (${VM_NAME})."
    else
        echo "Found ${SNAP_COUNT} snapshot(s) for VM ${VMID} (${VM_NAME}):"
        echo "${SNAPSHOTS}" | sed 's/^/  /'
    fi

    VERSION=$(grep -Po '^version = \K.*' Cargo.toml | sed 's/"//g' | head -1)
    GIT_SHA=$(git rev-parse --short HEAD)
    NEW_SNAP="v${VERSION//./-}-${GIT_SHA}-$(date +%Y%m%d-%H%M%S)"

    echo ""
    echo "This will:"
    if [[ "${SNAP_COUNT}" -gt 0 ]]; then
        echo "  1. Delete all ${SNAP_COUNT} existing snapshot(s)"
    fi
    echo "  2. Create new snapshot: ${NEW_SNAP}"
    echo ""
    read -rp "Proceed? [y/N] " REPLY
    if [[ ! "${REPLY}" =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 0
    fi

    # Delete all existing snapshots
    for snap in ${SNAPSHOTS}; do
        echo "Deleting snapshot: ${snap}"
        ssh ${REMOTE} "qm delsnapshot ${VMID} '${snap}'"
    done

    # Create new snapshot
    DESC="nifty-filter v${VERSION} (${GIT_SHA})"
    echo "Creating snapshot ${NEW_SNAP}..."
    ssh ${REMOTE} "qm snapshot ${VMID} '${NEW_SNAP}' --description '${DESC}'"
    echo "  Snapshot: ${NEW_SNAP}"
    echo "  Description: ${DESC}"
    echo "  Rollback: ssh ${REMOTE} qm rollback ${VMID} ${NEW_SNAP}"

# Set up infra bridge and router NIC for the services VM, then create it via nixos-vm-template.
# The services VM is managed by nixos-vm-template; this target handles the network plumbing.
pve-install-services pve_host ip bridge="vmbr2" vm_name="infra-services" router_vmid="101" pve_storage="local-lvm" pve_vmid="202":
    #!/usr/bin/env bash
    set -eo pipefail

    PVE_HOST="{{pve_host}}"
    REMOTE="root@${PVE_HOST}"
    VM_NAME="{{vm_name}}"
    ROUTER_VMID="{{router_vmid}}"
    BRIDGE="{{bridge}}"
    # Accept bare IP or CIDR; default to /24
    IP_RAW="{{ip}}"
    IP_ADDR="${IP_RAW%%/*}"
    STATIC_IP="${IP_ADDR}/24"
    # Derive gateway as .1 of the /24 subnet
    GATEWAY="$(echo "${IP_ADDR}" | sed 's/\.[0-9]*$/.1/')"

    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    if [ ! -f "${VM_TEMPLATE_DIR}/Justfile" ]; then
        echo "ERROR: nixos-vm-template not found at ${VM_TEMPLATE_DIR}"
        echo "Set NIXOS_VM_TEMPLATE to the correct path."
        exit 1
    fi

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-services-%C -o ControlPersist=60"

    echo "Connecting to ${PVE_HOST}..."
    ssh ${SSH_OPTS} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    # --- Create isolated bridge (no PVE IP — all access goes through router) ---
    if ! ssh ${SSH_OPTS} ${REMOTE} "ip link show ${BRIDGE}" &>/dev/null; then
        echo "Creating isolated bridge ${BRIDGE} on ${PVE_HOST}..."
        ssh ${SSH_OPTS} ${REMOTE} "printf '\nauto %s\niface %s inet manual\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0\n' '${BRIDGE}' '${BRIDGE}' >> /etc/network/interfaces && ifup ${BRIDGE}"
        echo "  ${BRIDGE} created."
    else
        echo "Bridge ${BRIDGE} already exists."
    fi

    # --- Add a NIC to the router VM on this bridge (if not already present) ---
    ROUTER_CONFIG=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${ROUTER_VMID}")
    if ! echo "${ROUTER_CONFIG}" | grep -q "bridge=${BRIDGE}"; then
        NEXT_NET=1
        while echo "${ROUTER_CONFIG}" | grep -q "^net${NEXT_NET}:"; do
            NEXT_NET=$((NEXT_NET + 1))
        done
        echo "Adding net${NEXT_NET} (bridge=${BRIDGE}) to router VM ${ROUTER_VMID}..."
        ssh ${SSH_OPTS} ${REMOTE} "qm set ${ROUTER_VMID} --net${NEXT_NET} virtio,bridge=${BRIDGE}"
        INFRA_MAC=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${ROUTER_VMID}" | grep "^net${NEXT_NET}:" | grep -oP 'virtio=\K[^,]+')
        echo "  Router NIC added (MAC: ${INFRA_MAC})."
        echo "  Add this to your nifty-filter.hcl:"
        echo ""
        echo "    vlan \"infra\" {"
        echo "      id = 2"
        echo "      interface {"
        echo "        mac  = \"${INFRA_MAC}\""
        echo "        name = \"infra\""
        echo "      }"
        echo "      ..."
        echo "    }"
        echo ""
    else
        echo "Router VM ${ROUTER_VMID} already has a NIC on ${BRIDGE}."
    fi

    # Close PVE SSH before handing off to nixos-vm-template
    ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true
    trap - EXIT

    # --- Create the services VM via nixos-vm-template (proxmox backend) ---
    echo ""
    echo "Creating services VM via nixos-vm-template..."
    cd "${VM_TEMPLATE_DIR}"
    # Pre-set VMID so proxmox backend doesn't prompt
    mkdir -p "${VM_TEMPLATE_DIR}/machines/${VM_NAME}"
    echo "{{pve_vmid}}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/vmid"

    # PVE firewall: allow only the ports needed by infra services
    printf '%s\n' 22 53 80 443 > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/tcp_ports"
    printf '%s\n' 53 123 > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/udp_ports"

    # /etc/hosts entries so services can resolve the router
    NIFTY_DOMAIN="${NIFTY_DOMAIN:-nifty.internal}"
    echo "${GATEWAY} router.${NIFTY_DOMAIN}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/hosts"

    # DNS: use the router's dnsmasq so services can resolve .internal domains
    echo "${GATEWAY}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/resolv.conf"

    BACKEND=proxmox PVE_HOST="${PVE_HOST}" PVE_STORAGE="{{pve_storage}}" PVE_DISK_FORMAT=raw \
        just create-batch "${VM_NAME}" "podman,nifty-services" "2048" "2" "8G" "bridge:${BRIDGE}" "${STATIC_IP},${GATEWAY}"

# Set up Step-CA VM on the infra bridge, then create it via nixos-vm-template.
# Deploy this BEFORE the router or infra-services VMs.
# The infra bridge (vmbr2) must already exist (run pve-install-services first, or create it manually).
pve-install-step-ca pve_host ip bridge="vmbr2" vm_name="infra-CA" router_vmid="101" pve_storage="local-lvm" pve_vmid="100":
    #!/usr/bin/env bash
    set -eo pipefail

    PVE_HOST="{{pve_host}}"
    REMOTE="root@${PVE_HOST}"
    VM_NAME="{{vm_name}}"
    BRIDGE="{{bridge}}"
    # Accept bare IP or CIDR; default to /24
    IP_RAW="{{ip}}"
    IP_ADDR="${IP_RAW%%/*}"
    STATIC_IP="${IP_ADDR}/24"
    GATEWAY="$(echo "${IP_ADDR}" | sed 's/\.[0-9]*$/.1/')"

    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    if [ ! -f "${VM_TEMPLATE_DIR}/Justfile" ]; then
        echo "ERROR: nixos-vm-template not found at ${VM_TEMPLATE_DIR}"
        echo "Set NIXOS_VM_TEMPLATE to the correct path."
        exit 1
    fi

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-step-ca-%C -o ControlPersist=60"

    echo "Connecting to ${PVE_HOST}..."
    ssh ${SSH_OPTS} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    # --- Ensure the infra bridge exists ---
    if ! ssh ${SSH_OPTS} ${REMOTE} "ip link show ${BRIDGE}" &>/dev/null; then
        echo "Creating isolated bridge ${BRIDGE} on ${PVE_HOST}..."
        ssh ${SSH_OPTS} ${REMOTE} "printf '\nauto %s\niface %s inet manual\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0\n' '${BRIDGE}' '${BRIDGE}' >> /etc/network/interfaces && ifup ${BRIDGE}"
        echo "  ${BRIDGE} created."
    else
        echo "Bridge ${BRIDGE} already exists."
    fi

    # --- Add a NIC to the router VM on this bridge (if not already present) ---
    if ssh ${SSH_OPTS} ${REMOTE} "qm config {{router_vmid}}" &>/dev/null; then
        ROUTER_CONFIG=$(ssh ${SSH_OPTS} ${REMOTE} "qm config {{router_vmid}}")
        if ! echo "${ROUTER_CONFIG}" | grep -q "bridge=${BRIDGE}"; then
            NEXT_NET=1
            while echo "${ROUTER_CONFIG}" | grep -q "^net${NEXT_NET}:"; do
                NEXT_NET=$((NEXT_NET + 1))
            done
            echo "Adding net${NEXT_NET} (bridge=${BRIDGE}) to router VM {{router_vmid}}..."
            ssh ${SSH_OPTS} ${REMOTE} "qm set {{router_vmid}} --net${NEXT_NET} virtio,bridge=${BRIDGE}"
            INFRA_MAC=$(ssh ${SSH_OPTS} ${REMOTE} "qm config {{router_vmid}}" | grep "^net${NEXT_NET}:" | grep -oP 'virtio=\K[^,]+')
            echo "  Router NIC added (MAC: ${INFRA_MAC})."
            echo "  Add this to your nifty-filter.hcl:"
            echo ""
            echo "    vlan \"infra\" {"
            echo "      id = 2"
            echo "      interface {"
            echo "        mac  = \"${INFRA_MAC}\""
            echo "        name = \"infra\""
            echo "      }"
            echo "      ..."
            echo "    }"
            echo ""
        else
            echo "Router VM {{router_vmid}} already has a NIC on ${BRIDGE}."
        fi
    else
        echo "Router VM {{router_vmid}} does not exist yet — skipping NIC addition."
        echo "The router will get an infra NIC automatically when pve-install runs."
    fi

    # Close PVE SSH before handing off to nixos-vm-template
    ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true
    trap - EXIT

    # --- Create the Step-CA VM via nixos-vm-template (proxmox backend) ---
    echo ""
    echo "Creating Step-CA VM via nixos-vm-template..."
    cd "${VM_TEMPLATE_DIR}"
    mkdir -p "${VM_TEMPLATE_DIR}/machines/${VM_NAME}"
    echo "{{pve_vmid}}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/vmid"

    # PVE firewall: Step-CA only needs SSH + ACME port
    printf '%s\n' 22 9443 > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/tcp_ports"
    printf '%s\n' > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/udp_ports"

    # /etc/hosts entries so Step-CA can resolve the router for ACME challenges
    NIFTY_DOMAIN="${NIFTY_DOMAIN:-nifty.internal}"
    echo "${GATEWAY} router.${NIFTY_DOMAIN}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/hosts"

    # DNS: use the router's dnsmasq so Step-CA can resolve .internal domains
    echo "${GATEWAY}" > "${VM_TEMPLATE_DIR}/machines/${VM_NAME}/resolv.conf"

    BACKEND=proxmox PVE_HOST="${PVE_HOST}" PVE_STORAGE="{{pve_storage}}" PVE_DISK_FORMAT=raw \
        just create-batch "${VM_NAME}" "podman,step-ca" "512" "1" "4G" "bridge:${BRIDGE}" "${STATIC_IP},${GATEWAY}"

# Copy TLS certs from Step-CA VM to router and infra-services VMs.
# Jump chain: workstation → PVE host → router (mgmt) → infra VLAN VMs.
pve-distribute-certs pve_host step_ca_ip="10.99.2.3" router_ip="10.99.0.1" services_ip="10.99.2.2":
    #!/usr/bin/env bash
    set -eo pipefail

    PVE_HOST="{{pve_host}}"
    STEP_CA_IP="{{step_ca_ip}}"
    ROUTER_IP="{{router_ip}}"
    SERVICES_IP="{{services_ip}}"

    # Jump chain: PVE host → router (mgmt bridge) → infra VLAN
    PVE="root@${PVE_HOST}"
    ROUTER="admin@${ROUTER_IP}"
    JUMP_TO_ROUTER="-J ${PVE}"
    JUMP_TO_INFRA="-J ${PVE},${ROUTER}"

    CA="admin@${STEP_CA_IP}"

    echo "=== Distributing TLS certificates from Step-CA (${STEP_CA_IP}) ==="
    echo "    Jump chain: ${PVE_HOST} → ${ROUTER_IP} → ${STEP_CA_IP}"
    echo ""

    # Helper: read a file from Step-CA via double jump
    ca_cat() { ssh ${JUMP_TO_INFRA} ${CA} "sudo cat $1"; }

    # --- Copy root CA cert to workstation (for Nix build) ---
    echo "Fetching root CA cert..."
    mkdir -p "certs/${PVE_HOST}"
    ca_cat /var/lib/step-ca/certs/root_ca.crt > "certs/${PVE_HOST}/step-ca-root.crt"
    echo "  Saved to certs/${PVE_HOST}/step-ca-root.crt"

    # --- Copy dashboard client cert to router ---
    echo "Copying dashboard client cert to router (${ROUTER_IP})..."
    ssh ${JUMP_TO_ROUTER} ${ROUTER} "sudo mkdir -p /var/lib/nifty-dashboard && sudo chown root:wheel /var/lib/nifty-dashboard && sudo chmod 755 /var/lib/nifty-dashboard"
    ca_cat /var/lib/step-ca/client-certs/dashboard/cert.pem | \
        ssh ${JUMP_TO_ROUTER} ${ROUTER} "sudo tee /var/lib/nifty-dashboard/client-cert.pem > /dev/null && sudo chmod 644 /var/lib/nifty-dashboard/client-cert.pem"
    ca_cat /var/lib/step-ca/client-certs/dashboard/key.pem | \
        ssh ${JUMP_TO_ROUTER} ${ROUTER} "sudo tee /var/lib/nifty-dashboard/client-key.pem > /dev/null && sudo chmod 600 /var/lib/nifty-dashboard/client-key.pem"
    ca_cat /var/lib/step-ca/certs/root_ca.crt | \
        ssh ${JUMP_TO_ROUTER} ${ROUTER} "sudo tee /var/lib/nifty-dashboard/step-ca-root.crt > /dev/null && sudo chmod 644 /var/lib/nifty-dashboard/step-ca-root.crt"
    # Clear cached ACME server cert so dashboard requests a fresh one from the new CA.
    ssh ${JUMP_TO_ROUTER} ${ROUTER} "sudo rm -f /var/lib/nifty-dashboard/tls-cache/cached_*"
    echo "  Dashboard certs + CA root installed on router (ACME cache cleared)."

    # --- Copy service-monitor + traefik client certs to infra-services ---
    if ssh ${JUMP_TO_INFRA} admin@${SERVICES_IP} "true" 2>/dev/null; then
        echo "Copying service-monitor client cert to infra-services (${SERVICES_IP})..."
        ca_cat /var/lib/step-ca/client-certs/service-monitor/cert.pem | \
            ssh ${JUMP_TO_INFRA} admin@${SERVICES_IP} "sudo mkdir -p /var/lib/service-monitor-certs && sudo tee /var/lib/service-monitor-certs/cert.pem > /dev/null"
        ca_cat /var/lib/step-ca/client-certs/service-monitor/key.pem | \
            ssh ${JUMP_TO_INFRA} admin@${SERVICES_IP} "sudo tee /var/lib/service-monitor-certs/key.pem > /dev/null && sudo chmod 600 /var/lib/service-monitor-certs/key.pem"
        echo "  Service-monitor certs installed."

        echo "Copying traefik client cert to infra-services (${SERVICES_IP})..."
        ca_cat /var/lib/step-ca/client-certs/traefik/cert.pem | \
            ssh ${JUMP_TO_INFRA} admin@${SERVICES_IP} "sudo mkdir -p /var/lib/traefik-certs && sudo tee /var/lib/traefik-certs/cert.pem > /dev/null"
        ca_cat /var/lib/step-ca/client-certs/traefik/key.pem | \
            ssh ${JUMP_TO_INFRA} admin@${SERVICES_IP} "sudo tee /var/lib/traefik-certs/key.pem > /dev/null && sudo chmod 600 /var/lib/traefik-certs/key.pem"
        echo "  Traefik certs installed."
    else
        echo "Infra-services VM (${SERVICES_IP}) not reachable — skipping."
        echo "Run this again after deploying infra-services."
    fi

    # --- Copy CA root cert into nixos-vm-template machine dirs for upgrades ---
    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    if [ -d "${VM_TEMPLATE_DIR}/machines" ]; then
        for vm in infra-CA infra-services; do
            if [ -d "${VM_TEMPLATE_DIR}/machines/${vm}" ]; then
                cp "certs/${PVE_HOST}/step-ca-root.crt" "${VM_TEMPLATE_DIR}/machines/${vm}/ca-cert.pem"
                echo "  CA cert copied to nixos-vm-template machines/${vm}/ca-cert.pem"
            fi
        done
    fi

    echo ""
    echo "Done. Root CA cert saved to certs/${PVE_HOST}/step-ca-root.crt"
    echo "Rebuild the router to trust it: just pve-upgrade ${PVE_HOST} 101 nifty-filter"
    echo "Or restart the dashboard if already rebuilt: ssh ${JUMP_TO_ROUTER} ${ROUTER} sudo systemctl restart nifty-dashboard"

# Upgrade Step-CA VM (delegates to nixos-vm-template proxmox backend)
pve-upgrade-step-ca pve_host vm_name="infra-CA" pve_storage="local-lvm":
    #!/usr/bin/env bash
    set -eo pipefail
    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    if [ ! -f "${VM_TEMPLATE_DIR}/Justfile" ]; then
        echo "ERROR: nixos-vm-template not found at ${VM_TEMPLATE_DIR}"
        echo "Set NIXOS_VM_TEMPLATE to the correct path."
        exit 1
    fi
    cd "${VM_TEMPLATE_DIR}"
    BACKEND=proxmox PVE_HOST="{{pve_host}}" PVE_STORAGE="{{pve_storage}}" PVE_DISK_FORMAT=raw just upgrade "{{vm_name}}"

# Upgrade infra-services VM (delegates to nixos-vm-template proxmox backend)
pve-upgrade-services pve_host vm_name="infra-services" pve_storage="local-lvm":
    #!/usr/bin/env bash
    set -eo pipefail
    VM_TEMPLATE_DIR="${NIXOS_VM_TEMPLATE:-$(cd .. && pwd)/nixos-vm-template}"
    if [ ! -f "${VM_TEMPLATE_DIR}/Justfile" ]; then
        echo "ERROR: nixos-vm-template not found at ${VM_TEMPLATE_DIR}"
        echo "Set NIXOS_VM_TEMPLATE to the correct path."
        exit 1
    fi
    cd "${VM_TEMPLATE_DIR}"
    BACKEND=proxmox PVE_HOST="{{pve_host}}" PVE_STORAGE="{{pve_storage}}" PVE_DISK_FORMAT=raw just upgrade "{{vm_name}}"

# Clean all artifacts
clean *args: clean-profile
    cargo clean {{args}}

# Clean profile artifacts only
clean-profile:
    rm -rf *.profraw *.profdata

# Clean old nix ISO builds
clean-nix:
    rm -f result
    nix-collect-garbage -d
