# nifty-filter

nifty-filter is a declarative config to deploy routers and firewalls. It is two things that share the same name:

1. **A standalone nftables rule generator** — a Rust library and CLI tool that reads
   env vars (or a `.env` file) and emits a complete nftables ruleset.
   Install it with `cargo install nifty-filter` and use it as a standalone peice in your own adhoc router.
   
2. **A declarative NixOS router distribution** — a complete router operating system, built around that same binary, with DHCP,
   DNS, VLANs, an interactive installer, and more. Deploy a fully featured router on Proxmox VE, or bare metal.

nifty-filter is in a stage of development and should be used for research purposes
only. Use it at your own risk!

## Standalone usage

```bash
cargo install nifty-filter
```

Or [download a release](https://github.com/EnigmaCurry/nifty-filter/releases).

```bash
# Generate rules from an env file:
nifty-filter nftables --env-file router.env --strict-env

# Generate and validate (requires nft on the host):
nifty-filter nftables --env-file router.env --strict-env --validate

# From environment variables:
INTERFACE_LAN=lan INTERFACE_WAN=wan SUBNET_LAN=10.99.0.1/24 \
  nifty-filter nftables
```

The ruleset is generated from a compile-time validated
[askama template](templates/router.nft.txt). See
[examples/](examples/) for complete configurations covering a basic
home router, dual-stack IPv6, and multi-VLAN setups.

---

# NixOS Router Distribution

nifty-filter as a NixOS distribution provides DHCP, DNS, VLANs,
firewall, and an interactive configuration TUI — all driven by env
files on the writable `/var` partition. The root filesystem is
read-only.

Install it on Proxmox VE (preferred for backups and QoL) or
bare metal. You can use virtual NICs (for routing other VMs) or real
network hardware via PCI passthrough. A full router on a hypervisor
platform is useful to put your apps right on the edge of your network.

## Prerequisites

 * You need a workstation (Linux/macOS) to build images.
 * You need a separate Proxmox VE host (or bare metal target) to run the router.

On your workstation:

 * Clone this repo.
 * Install [Nix](https://nixos.org/download/) and [just](https://github.com/casey/just).
 * Ensure your ssh-agent is running and that `ssh-add -L` returns at least one loaded key.
 * Ensure you can login to your PVE host (test `ssh root@<proxmox-host> whoami`).

## Justfile commands

Run `just help` to list all available targets. The key commands are
summarized below.

### Build

| Command | Description |
|---------|-------------|
| `just pve-image` | Build a PVE disk image (pre-partitioned, ready to import) |
| `just iso` | Build the NixOS router ISO image (fits on a CD-ROM or any USB) |
| `just iso-big` | Build ISO with full hardware support (linux-firmware + all drivers; fits on a DVD) |
| `just clean-nix` | Remove build artifacts and run garbage collection |

### Proxmox VE lifecycle

These commands manage the full VM lifecycle on a Proxmox VE host. Each
`pve-*` command that takes a `vmid` and `vm_name` verifies the name
matches before acting, so you cannot accidentally destroy the wrong VM.

| Command | Description |
|---------|-------------|
| `just pve-install <pve-host>` | Interactive: build disk image, upload, create and start a VM |
| `just pve-ssh <pve-host> <target-ip> [user]` | SSH to VM via PVE jump host |
| `just pve-start <pve-host> <vmid> <vm-name>` | Start a VM |
| `just pve-stop <pve-host> <vmid> <vm-name>` | Gracefully stop a VM |
| `just pve-destroy <pve-host> <vmid> <vm-name>` | Destroy a VM (stops first if running) |

### Upgrade and maintenance

| Command | Description |
|---------|-------------|
| `just upgrade <router-ip>` | Build locally, rsync to router, reboot |

## Deploying to Proxmox VE

This walkthrough covers a full deployment from scratch. The example
uses PCI passthrough NICs, but virtual bridge NICs (`vmbr*`) or a mix
of both also work.

### 1. Destroy any existing VM (optional)

If you are rebuilding, tear down the old VM first:

```bash
just pve-destroy pve-router 100 nifty-filter
```

### 2. Create and boot the VM

`pve-install` is interactive — it connects to the PVE host, queries
available VM IDs and network devices, and walks you through the setup:

```bash
just pve-install pve-router
```

The wizard will prompt for:
- **VM ID** — defaults to the lowest unused ID (starting at 100)
- **VM name** — defaults to `nifty-filter`
- **WAN NIC** — choose virtual (bridge) or PCI passthrough
- **LAN NIC** — same choice
- **Additional NICs** — add as many as needed

For PCI passthrough, it lists all network PCI devices on the host. For
virtual NICs, it lists available bridges. Set `MGMT_SUBNET` to
override the management subnet (default: `10.99.0.0/24`).

A dedicated `mgmt` bridge is always created automatically for
out-of-band management (default subnet `10.99.0.0/24`). The VM gets
two disks: a boot+root disk (read-only NixOS system) and a `/var` disk
(writable config/state). It is created with q35/UEFI, 2 cores, 2 GB
RAM, and serial console (no VGA). Set `VAR_SIZE` to override the
default 8 GB `/var` disk.

### 3. SSH in and configure

SSH keys are pre-installed (collected from your workstation agent and
the PVE host's `/root/.ssh/`) — connect directly:

```bash
just pve-ssh pve-router 10.99.0.1
```

Run the configuration wizard:

```bash
nifty-install
```

The wizard prompts for hostname, WAN/LAN interfaces, VLANs, subnets,
DHCP pools, and DNS servers. It writes the configuration and reboots
(interface renaming requires a reboot to take effect).

## Deploying to bare metal

### 1. Build and flash the ISO

```bash
just iso
sudo dd if=result/iso/nifty-filter-*.iso of=/dev/sdX bs=4M status=progress
```

Use `just iso-big` if you need full hardware support (all
linux-firmware and drivers).

### 2. Boot, install, and configure

Boot from the media. The console shows the IP address and login
credentials (`admin` / `nifty`). Copy your SSH key and connect:

```bash
ssh-copy-id admin@<router-ip>
ssh admin@<router-ip>
```

Run `nifty-install`, then remove the media and power on.

## Configuring the router

SSH into the installed system and use the interactive menu:

```bash
nifty-config
```

Or edit the env file directly:

```bash
nano /var/nifty-filter/nifty-filter.env
```

Apply changes without rebooting:

```bash
sudo systemctl restart nifty-filter   # Firewall rules
sudo systemctl restart nifty-dnsmasq  # DHCP/DNS
```

## Upgrading

### From a workstation

```bash
just upgrade <router-ip>
```

Builds the system closure locally, rsyncs only the missing store paths
to the router over SSH, updates boot entries, and reboots.

### From the router

```bash
nifty-upgrade
```

Pulls the latest source from git, builds on the router, and reboots.

## Maintenance mode

```bash
nifty-maintenance
```

Reboots into a one-shot mode with the root filesystem mounted
read-write. The console auto-logs in with a red `[MAINTENANCE]`
prompt. Reboot again to return to normal read-only mode.

## System architecture

### Filesystem layout

| Mount | Mode | Purpose |
|-------|------|---------|
| `/` | read-only | NixOS system, nifty-filter binary, all services |
| `/var` | read-write | Router config, DHCP leases, SSH keys, logs |
| `/boot` | read-write | EFI system partition, kernel, bootloader |
| `/tmp` | tmpfs | Scratch (cleared on reboot) |
| `/home` | bind from `/var/home` | User home directories |

### Boot services

| Service | Purpose | Config source |
|---------|---------|---------------|
| `nifty-link` | Renames interfaces by MAC address | `nifty-filter.env` |
| `nifty-hostname` | Sets hostname | `nifty-filter.env` |
| `nifty-network` | Configures WAN (DHCP) and LAN (static IP) | `nifty-filter.env` |
| `nifty-filter-init` | Seeds default config on first boot | -- |
| `nifty-filter` | Generates and applies nftables rules | `nifty-filter.env` |
| `nifty-dnsmasq` | DHCP and DNS server | `nifty-filter.env` |

### Configuration files

All config lives in `/var/nifty-filter/`:

```
/var/nifty-filter/
  nifty-filter.env        # All router config (firewall, interfaces, DHCP, DNS)
  ssh/
    ssh_host_*            # Persistent SSH host keys
```

### Default firewall rules

 * Default deny on input, forward, and output chains
 * Stateful connection tracking (established/related)
 * LAN-to-WAN forwarding with masquerade NAT
 * SSH (port 22) accepted on both WAN and LAN
 * DHCP (ports 67/68) accepted on LAN
 * ICMP echo accepted on LAN
 * Configurable port forwarding (DNAT)

