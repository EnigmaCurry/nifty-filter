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

# Build NixOS router ISO image
iso:
    NIFTY_BUILD_BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo master)" nix build .#iso --impure
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
    NIFTY_BUILD_BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo master)" nix build .#iso-big --impure
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

    # Check if remote is already running this system
    CURRENT=$(ssh ${SSH_OPTS} ${REMOTE} readlink -f /nix/var/nix/profiles/system 2>/dev/null || echo "")
    if [ "${CURRENT}" = "${SYSTEM_PATH}" ] && [ "${MISSING_COUNT}" -eq 0 ]; then
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
    sudo mount -o remount,ro /nix/store
    sudo mount -o remount,ro /
    nohup sudo reboot &>/dev/null &
    REMOTE_SCRIPT
    echo ""
    echo "Upgrade applied. {{host}} is rebooting..."

# Create a NixOS router VM on Proxmox VE (PCI passthrough and/or virtual NICs)
# A dedicated 'mgmt' bridge is always created for out-of-band management.
pve-install pve_host vmid name +nics:
    #!/usr/bin/env bash
    set -eo pipefail
    source ./funcs.sh

    PVE_HOST="{{pve_host}}"
    VMID="{{vmid}}"
    VM_NAME="{{name}}"
    NICS=({{nics}})
    MGMT_SUBNET="${MGMT_SUBNET:-10.99.0.0/24}"

    if [ "${#NICS[@]}" -lt 1 ]; then
        echo "Usage: just pve-install <pve-host> <vmid> <name> <nic> [<nic>...]"
        echo ""
        echo "Each <nic> is either a PCI device ID or a bridge name (vmbr*)."
        echo "A dedicated 'mgmt' bridge and NIC are always added automatically."
        echo ""
        echo "Set MGMT_SUBNET to override the management subnet (default: 10.99.0.0/24)."
        echo ""
        echo "Examples:"
        echo "  just pve-install pve.local 100 nifty-filter vmbr0 vmbr1        # virtual NICs"
        echo "  just pve-install pve.local 100 nifty-filter 01:00              # multi-port PCI NIC"
        echo "  just pve-install pve.local 100 nifty-filter 01:00.0 02:00.0    # two PCI NICs"
        echo "  just pve-install pve.local 100 nifty-filter vmbr0 01:00.0      # mixed"
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

    # --- Build or reuse ISO ---
    HEAD_SHORT="$(git rev-parse --short HEAD)"
    ISO_PATH=""
    REBUILD=true
    if ls result/iso/nifty-filter-*.iso 1>/dev/null 2>&1; then
        ISO_PATH="$(readlink -f result/iso/nifty-filter-*.iso)"
        ISO_BASENAME="$(basename "${ISO_PATH}")"
        if echo "${ISO_BASENAME}" | grep -q "${HEAD_SHORT}"; then
            echo "Found ISO matching HEAD (${HEAD_SHORT}): ${ISO_BASENAME}"
            REBUILD=false
        else
            echo "Found stale ISO (HEAD is ${HEAD_SHORT}): ${ISO_BASENAME}"
            echo "Rebuilding..."
        fi
    else
        echo "No existing ISO found. Building..."
    fi
    if [ "${REBUILD}" = true ]; then
        just iso
        ISO_PATH="$(readlink -f result/iso/nifty-filter-*.iso)"
    fi
    ISO_FILENAME="$(basename "${ISO_PATH}")"
    echo "Using ISO: ${ISO_FILENAME}"

    # --- Connect to PVE and upload ISO ---
    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-pve-%C -o ControlPersist=60"
    REMOTE="root@${PVE_HOST}"

    echo "Connecting to ${PVE_HOST}..."
    ssh ${SSH_OPTS} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    echo "Uploading ISO to ${PVE_HOST}:/var/lib/vz/template/iso/${ISO_FILENAME} ..."
    rsync -ah --progress -e "ssh ${SSH_OPTS}" \
        "${ISO_PATH}" \
        "${REMOTE}:/var/lib/vz/template/iso/${ISO_FILENAME}"
    echo "ISO uploaded."

    # --- Create mgmt bridge (always) and any user-specified bridges ---
    ALL_BRIDGES=("mgmt" "${BRIDGES[@]}")
    for bridge in "${ALL_BRIDGES[@]}"; do
        if ! ssh ${SSH_OPTS} ${REMOTE} "ip link show ${bridge}" &>/dev/null; then
            echo "Creating isolated bridge ${bridge} on ${PVE_HOST}..."
            ssh ${SSH_OPTS} ${REMOTE} "printf '\nauto %s\niface %s inet manual\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0\n' '${bridge}' '${bridge}' >> /etc/network/interfaces && ifreload -a"
            echo "  ${bridge} created."
        fi
    done

    # --- Build NIC flags (mgmt is always net0) ---
    NIC_ARGS="--net0 virtio,bridge=mgmt"
    NET_INDEX=1
    for bridge in "${BRIDGES[@]}"; do
        NIC_ARGS="${NIC_ARGS} --net${NET_INDEX} virtio,bridge=${bridge}"
        NET_INDEX=$((NET_INDEX + 1))
    done

    HOSTPCI_ARGS=""
    PCI_INDEX=0
    for dev in "${PCI_DEVICES[@]}"; do
        HOSTPCI_ARGS="${HOSTPCI_ARGS} --hostpci${PCI_INDEX} 0000:${dev},pcie=1"
        PCI_INDEX=$((PCI_INDEX + 1))
    done

    # Use q35 machine type if any PCI passthrough, otherwise default i440fx
    MACHINE="q35"

    # --- Create VM ---
    echo "Creating VM ${VMID} (${VM_NAME}) on ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${REMOTE} "qm create ${VMID} \
        --name ${VM_NAME} \
        --machine ${MACHINE} \
        --bios ovmf \
        --cpu host \
        --cores 2 \
        --memory 2048 \
        --efidisk0 local-lvm:1,efitype=4m,pre-enrolled-keys=0 \
        --scsi0 local-lvm:16 \
        --scsihw virtio-scsi-single \
        --ide2 local:iso/${ISO_FILENAME},media=cdrom \
        --boot order=ide2\;scsi0 \
        --ostype l26 \
        --onboot 1 \
        --serial0 socket \
        --vga serial0 \
        ${NIC_ARGS} ${HOSTPCI_ARGS}"
    echo "VM ${VMID} created."

    # --- Query MAC addresses for mgmt and WAN ---
    # mgmt is always net0 (virtio)
    MGMT_MAC=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${VMID}" | grep '^net0:' | grep -oP 'virtio=\K[^,]+')
    echo "  mgmt MAC: ${MGMT_MAC}"

    # WAN is the first user-specified NIC
    FIRST_NIC="${NICS[0]}"
    if [[ "${FIRST_NIC}" == vmbr* ]]; then
        # WAN is a bridge (net1)
        WAN_MAC=$(ssh ${SSH_OPTS} ${REMOTE} "qm config ${VMID}" | grep '^net1:' | grep -oP 'virtio=\K[^,]+')
    else
        # WAN is a PCI passthrough device — read MAC from host sysfs
        WAN_PCI="0000:${FIRST_NIC#0000:}"
        WAN_MAC=$(ssh ${SSH_OPTS} ${REMOTE} "cat /sys/bus/pci/devices/${WAN_PCI}/net/*/address 2>/dev/null | head -1")
        if [ -z "${WAN_MAC}" ]; then
            echo "WARNING: Could not read MAC for PCI device ${WAN_PCI}. WAN identification may fail on the ISO."
        fi
    fi
    echo "  WAN MAC:  ${WAN_MAC}"

    # --- Pass MAC addresses to the VM via fw_cfg ---
    FW_CFG_ARGS="-fw_cfg name=opt/nifty/mgmt_mac,string=${MGMT_MAC}"
    if [ -n "${WAN_MAC}" ]; then
        FW_CFG_ARGS="${FW_CFG_ARGS} -fw_cfg name=opt/nifty/wan_mac,string=${WAN_MAC}"
    fi
    ssh ${SSH_OPTS} ${REMOTE} "qm set ${VMID} --args '${FW_CFG_ARGS}'"

    # --- Add PVE host IP to mgmt bridge ---
    echo "Adding ${PVE_MGMT_IP} to mgmt bridge on ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${REMOTE} "ip addr add ${PVE_MGMT_IP} dev mgmt 2>/dev/null || true"

    # --- Start VM ---
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
    for dev in "${PCI_DEVICES[@]}"; do
        echo "  PCI:     0000:${dev}"
    done
    echo ""
    echo "Starting VM ${VMID}..."
    ssh ${SSH_OPTS} ${REMOTE} "qm start ${VMID}"
    echo "VM ${VMID} started."
    echo ""
    echo "Connect to the live ISO from the PVE host:"
    echo "  ssh admin@${ROUTER_MGMT_IP}"
    echo ""
    echo "After NixOS installs to disk, eject the ISO and reboot:"
    echo "  just pve-eject-iso ${PVE_HOST} ${VMID}"

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
