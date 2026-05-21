# nifty-filter

nifty-filter is a declarative config to deploy network routers and firewalls. It is two things that share the same name:

1. **A standalone nftables rule generator** — a Rust library and CLI tool that reads
   an HCL config file and emits a complete nftables ruleset.
   Install it with `cargo install nifty-filter` and use it as a standalone piece in your own adhoc router.
   
2. **A declarative NixOS router distribution** — a complete router operating system, built around that same binary, with DHCP,
   DNS, VLANs, an interactive installer, and more. Deploy a fully featured router on Proxmox VE.

nifty-filter is in a stage of development and should be used for research purposes
only. Use it at your own risk!

## Standalone usage

```bash
cargo install nifty-filter
```

Or [download a release](https://github.com/EnigmaCurry/nifty-filter/releases).

```bash
# Generate rules from an HCL config file:
nifty-filter nftables --config router.hcl

# Generate and validate (requires nft on the host):
nifty-filter nftables --config router.hcl --validate

# Generate QoS (CAKE) traffic shaping commands:
nifty-filter qos --config router.hcl
```

The ruleset is generated from a compile-time validated
[askama template](templates/router.nft.txt). See
[examples/](examples/) for complete configurations covering a basic
home router, dual-stack IPv6, and multi-VLAN setups.

### HCL configuration

nifty-filter uses [HCL](https://github.com/hashicorp/hcl) (HashiCorp
Configuration Language) for its config format. HCL provides labeled
blocks, real lists, typed values, and comments — making network config
readable and structured.

```hcl
interfaces {
  trunk = "trunk"
  wan   = "wan"
}

wan {
  enable_ipv4 = true
  enable_ipv6 = true
  tcp_forward = ["443:10.99.40.50:443"]
}

vlan "trusted" {
  id = 10
  ipv4 {
    subnet = "10.99.10.1/24"
    egress = ["0.0.0.0/0"]
  }
  firewall {
    tcp_accept = [22]
    udp_accept = [53, 67, 68]
  }
  dhcp {
    pool_start = "10.99.10.100"
    pool_end   = "10.99.10.250"
    router     = "10.99.10.1"
    dns        = "10.99.10.1"
  }
}
```

See [examples/home_router.hcl](examples/home_router.hcl) for a simple
setup, [examples/dual_stack_router.hcl](examples/dual_stack_router.hcl)
for IPv4+IPv6, and
[examples/vlan_router.hcl](examples/vlan_router.hcl) for a full
multi-VLAN configuration with managed switch.

---

# NixOS Router Distribution

nifty-filter as a NixOS distribution provides DHCP, DNS, VLANs,
firewall, and an interactive configuration TUI — all driven by config
files on the writable `/var` partition. The root filesystem is
read-only.

Install it on Proxmox VE (preferred for backups and QoL) or
bare metal. You can use virtual NICs (for routing other VMs) or real
network hardware via PCI passthrough. A full router on a hypervisor
platform is useful to put your apps right on the edge of your network.

## Prerequisites

 * You need a workstation (Linux/macOS) to build images.
 * You need a separate Proxmox VE host to run the router.

On your workstation:

 * Clone this repo.
 * Install [Nix](https://nixos.org/download/) and [just](https://github.com/casey/just).
 * Ensure your ssh-agent is running and that `ssh-add -L` returns at least one loaded key.
 * Ensure you can login to your PVE host (test `ssh root@<proxmox-host> whoami`).

## Example network

The rest of this document uses the
[`examples/vlan_router.hcl`](examples/vlan_router.hcl) config as a
running example. It defines a VLAN-aware router with five VLANs behind
a managed switch:

| VLAN | ID | Subnet | Purpose |
|------|----|--------|---------|
| infra | 2 | `10.99.2.0/24` | Infrastructure services (Step-CA, Traefik, DNS, NTP). Uses a dedicated virtual NIC on an isolated bridge — not on the trunk/switch. |
| trusted | 10 | `10.99.10.0/24` | Trusted devices. Full internet, SSH to router, dashboard access. mDNS reflected to IoT. |
| iot | 20 | `10.99.20.0/24` | IoT jail. DHCP only, no internet, no router access beyond DHCP/DNS. mDNS reflected to trusted. |
| guest | 30 | `10.99.30.0/24` | Guest network. Internet access but no SSH or dashboard. |
| lab | 40 | `10.99.40.0/24` + `fd00:40::/64` | Lab (dual-stack). Full internet on IPv4 and IPv6, SSH to router. |

The management interface (`10.99.0.0/24`) provides out-of-band access
to the router from the Proxmox host.

## Deploying to Proxmox VE

A full deployment consists of three VMs, deployed in order.

| VM | VMID | IP | Purpose |
|----|------|----|---------|
| infra-CA | 100 | 10.99.2.3 | Step-CA private PKI (ACME + mTLS certs) |
| nifty-filter | 101 | 10.99.0.1 | Router, firewall, dashboard |
| infra-services | 202 | 10.99.2.2 | Traefik, Technitium DNS, DDNS, NTP |

Splitting the infrastructure across separate VMs provides several
benefits: each VM can be upgraded and rebooted independently without
disrupting the others, smaller images build and boot faster, and
kernel-level isolation limits the blast radius if any single service is
compromised. Because these VMs communicate over the network, they need
a way to cryptographically verify each other's identity. Step-CA
provides a private certificate authority that issues TLS server
certificates (via ACME) and mTLS client certificates so that every
connection between VMs is authenticated. The CA runs on its own VM so
that its private keys are never co-located with application workloads.
With long-lived certificates, the CA VM can be shut down entirely
between renewals to further limit access to the CA.

The router uses PCI passthrough NICs for WAN, trunk, and management.
The infra VLAN uses a virtual NIC on an isolated bridge (`vmbr2`)
shared between the router and infrastructure VMs.

### 1. Test the PVE connection

All `pve-*` commands take an SSH host alias as the first argument. Add
an entry to your `~/.ssh/config`:

```
Host pve-router
    HostName 192.168.1.100   # your Proxmox host IP on the management interface (vmbr0)
    User root
```

Then verify the connection:

```bash
just pve-status pve-router
```

This prints the logged-in user, PVE version, uptime, and any existing
VMs.

### 2. Deploy Step-CA (infra-CA)

The CA VM deploys first — it has no external dependencies (the
container image is built by Nix, no registry pull needed).

```bash
just pve-install-step-ca pve-router 10.99.2.3
```

This creates the infra bridge (`vmbr2`), adds a NIC to the router VM
slot, and creates a minimal VM (1 CPU, 512 MB, 4 GB disk). On first
boot, Step-CA bootstraps automatically: generates a root CA, enables
ACME, and issues client certificates for dashboard, service-monitor,
and traefik.

After boot, copy the root CA cert and client certs from the CA VM:

```bash
# Root CA cert (add to your Nix config for security.pki.certificateFiles)
scp user@10.99.2.3:/var/lib/step-ca/certs/root_ca.crt ./

# Dashboard client cert (copy to router VM later)
scp user@10.99.2.3:/var/lib/step-ca/client-certs/dashboard/cert.pem ./dashboard-cert.pem
scp user@10.99.2.3:/var/lib/step-ca/client-certs/dashboard/key.pem ./dashboard-key.pem

# Service-monitor + traefik client certs (copy to infra-services VM later)
scp user@10.99.2.3:/var/lib/step-ca/client-certs/service-monitor/ ./
scp user@10.99.2.3:/var/lib/step-ca/client-certs/traefik/ ./
```

### 3. Deploy the router

```bash
just pve-install pve-router
```

The wizard will prompt for:
- **VM ID** — defaults to the lowest unused ID (starting at 101)
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

### 4. Configure the router

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

To enable ACME + mTLS, copy the dashboard client cert/key to the
router and add a `dashboard_tls` block to the HCL config:

```hcl
dashboard_tls {
  acme_directory_url = "https://10.99.2.3:9443/acme/acme/directory"
  client_cert        = "/var/lib/nifty-dashboard/client-cert.pem"
  client_key         = "/var/lib/nifty-dashboard/client-key.pem"
  sans               = ["router.nifty.internal"]
}
```

Then reboot. The dashboard will obtain its server cert from Step-CA via
ACME and require mTLS client certificates on all HTTPS endpoints.

### 5. Deploy infra-services

With the router online as a gateway, deploy the services VM:

```bash
just pve-install-services pve-router 10.99.2.2
```

This creates the infra-services VM (2 CPU, 2 GB, 8 GB disk) with
Traefik, Technitium DNS, DDNS updater, Chrony NTP, and the
service-monitor. Container images are pulled from the registry via the
router gateway.

### Upgrading

Rebuild and upgrade any VM in place (preserves `/var`):

```bash
just pve-upgrade-step-ca pve-router      # Upgrade Step-CA
just pve-upgrade pve-router              # Upgrade router
just pve-upgrade-services pve-router     # Upgrade infra-services
```

### Destroying VMs

```bash
just pve-destroy pve-router 100 infra-CA        # Step-CA
just pve-destroy pve-router 101 nifty-filter     # Router
just pve-destroy pve-router 202 infra-services   # Services
```

## Configuring the router

SSH into the installed system and edit the HCL config:

```bash
nano /var/nifty-filter/nifty-filter.hcl
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
| `nifty-link` | Renames interfaces by MAC address | `nifty-filter.hcl` |
| `nifty-hostname` | Sets hostname | `nifty-filter.hcl` |
| `nifty-network` | Configures WAN (DHCP) and LAN (static IP) | `nifty-filter.hcl` |
| `nifty-filter-init` | Seeds default config on first boot | -- |
| `nifty-filter` | Generates and applies nftables rules | `nifty-filter.hcl` |
| `nifty-dnsmasq` | DHCP and DNS server | `nifty-filter.hcl` |

### Configuration files

All config lives in `/var/nifty-filter/`:

```
/var/nifty-filter/
  nifty-filter.hcl        # All router config (firewall, interfaces, DHCP, DNS)
  ssh/
    ssh_host_*            # Persistent SSH host keys
```
