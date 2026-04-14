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
    nix build .#iso
    @echo ""
    @echo "ISO built successfully:"
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
    else
        echo "All store paths already present."
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
    sudo mount -o remount,ro /nix/store
    sudo mount -o remount,ro /
    REMOTE_SCRIPT
    echo ""
    echo "Upgrade staged on {{host}}."
    echo "Reboot to apply: ssh ${SSH_OPTS} ${REMOTE} sudo reboot"

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
