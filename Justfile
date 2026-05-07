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

# Build PVE disk image (pre-partitioned, ready to import)
pve-image:
    #!/usr/bin/env bash
    set -eo pipefail
    KEYS="$(ssh-add -L 2>/dev/null || true)"
    if [ -z "${KEYS}" ]; then
        echo "ERROR: No keys found in SSH agent (ssh-add -L returned nothing)"
        exit 1
    fi
    export NIFTY_SSH_KEYS="${KEYS}"
    NIFTY_BUILD_BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo master)" \
        nix build .#pve-image --impure
    echo ""
    echo "PVE disk image built successfully:"
    echo "  $(ls result/)"

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

# Upgrade a remote router VM via PVE jump host (builds locally, stages for next reboot)
pve-upgrade pve_host vmid vm_name target_ip="10.99.0.1":
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

    SSH_OPTS="-o ControlMaster=auto -o ControlPath=/tmp/nifty-pve-upgrade-%C -o ControlPersist=60 -o ServerAliveInterval=30"

    # Open persistent SSH connection through PVE jump host (authenticates once)
    echo "Connecting to {{target_ip}} via ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${PROXY} -fN ${REMOTE}
    trap 'ssh ${SSH_OPTS} ${PROXY} -O exit ${REMOTE} 2>/dev/null || true' EXIT

    echo "Building system closure..."
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
    cargo build --quiet --features nixos 2>/dev/null
    SETUP_OUTPUT=$(cargo run --quiet --features nixos -- pve-setup "${PVE_HOST}")
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

    # --- Collect SSH keys ---
    echo "Collecting SSH keys..."
    WORKSTATION_KEYS="$(ssh-add -L 2>/dev/null || true)"
    if [ -z "${WORKSTATION_KEYS}" ]; then
        echo "ERROR: No keys found in SSH agent (ssh-add -L returned nothing)"
        exit 1
    fi
    PVE_KEYS="$(ssh ${SSH_OPTS} ${REMOTE} 'cat /root/.ssh/id_ed25519.pub /root/.ssh/id_rsa.pub 2>/dev/null || true')"
    NIFTY_SSH_KEYS="$(echo -e "${WORKSTATION_KEYS}\n${PVE_KEYS}" | sort -u | grep -v '^$')"
    export NIFTY_SSH_KEYS
    KEY_COUNT=$(echo "${NIFTY_SSH_KEYS}" | wc -l)
    echo "  ${KEY_COUNT} SSH key(s) collected"

    # --- Build PVE disk image ---
    echo "Building PVE disk image..."
    NIFTY_BUILD_BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo master)" \
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

    ssh ${SSH_OPTS} ${REMOTE} "qm set ${VMID} --args '${FW_CFG_ARGS}'"

    # --- Add PVE host IP to mgmt bridge ---
    echo "Adding ${PVE_MGMT_IP} to mgmt bridge on ${PVE_HOST}..."
    ssh ${SSH_OPTS} ${REMOTE} "ip addr add ${PVE_MGMT_IP} dev mgmt 2>/dev/null || true"

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
    for dev in "${PCI_DEVICES[@]}"; do
        echo "  PCI:     0000:${dev}"
    done
    echo ""
    echo "SSH keys are pre-installed. Connect directly:"
    echo "  just pve-ssh ${PVE_HOST} ${ROUTER_MGMT_IP}"
    echo ""
    echo "Then run the configuration wizard:"
    echo "  nifty-install"

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
