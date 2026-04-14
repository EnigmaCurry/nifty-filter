# nifty-filter

nifty-filter is a template and configuration tool for
[netfilter](https://www.netfilter.org/) (nftables) and is useful for
creating a Linux based Internet protocol (IP) router. It is a program
that generates the `nftables.nft` config file, using its own internal
template. The configuration is done entirely by environment variables
(or `.env` file) and the output is type checked and validated.

The jinja-like [template](templates/router.nft.txt) is powered by
[djc/askama](https://github.com/djc/askama), which implements compile
time type checking of input values. Therefore, if you wish to
customize the template, you will have to compile your own nifty-filter
binary. However, the default template is designed to cover most of the
use cases for a typical home LAN router, so if that suits your needs
then you can simply download the precompiled binary from the releases
page.

## NixOS router

nifty-filter includes a NixOS flake that builds an immutable router
system. The root filesystem is read-only. All runtime configuration
lives on the writable `/var` partition as an env file. Edit it and
reboot to apply.

### Build the ISO

```bash
nix build .#iso
```

The ISO image will be at `result/iso/nifty-filter-*.iso`. Flash it to
a USB drive:

```bash
sudo dd if=result/iso/nifty-filter-*.iso of=/dev/sdX bs=4M status=progress
```

### Boot and initial setup

Boot from the USB. Log in on the console:

 * Username: `admin`
 * Password: `nifty`

Identify your network interfaces:

```bash
ip link
```

### Configure the router

Edit the env file on the writable `/var` partition:

```bash
sudo vim /var/nifty-filter/router.env
```

A default env file is seeded on first boot. It looks like this:

```bash
# Network interfaces
INTERFACE_LAN=enp1s0
INTERFACE_WAN=enp2s0

# LAN subnet (router's LAN IP / prefix length)
SUBNET_LAN=192.168.10.1/24

# ICMP types accepted on each interface
ICMP_ACCEPT_LAN=echo-request,echo-reply,destination-unreachable,time-exceeded
ICMP_ACCEPT_WAN=

# TCP/UDP ports the router accepts directly
TCP_ACCEPT_LAN=22
UDP_ACCEPT_LAN=
TCP_ACCEPT_WAN=
UDP_ACCEPT_WAN=

# Port forwarding (format: incoming_port:destination_ip:destination_port)
TCP_FORWARD_LAN=
UDP_FORWARD_LAN=
TCP_FORWARD_WAN=
UDP_FORWARD_WAN=
```

Change `INTERFACE_LAN` and `INTERFACE_WAN` to match the interfaces you
identified with `ip link`. Adjust the subnet, ports, and forwarding
rules as needed.

### Apply changes

Reboot to apply:

```bash
sudo reboot
```

Or apply without rebooting:

```bash
sudo systemctl restart nifty-filter
```

### System architecture

| Mount | Mode | Purpose |
|-------|------|---------|
| `/` | read-only | NixOS system, nifty-filter binary, all services |
| `/var` | read-write | Router config, DHCP leases, logs |
| `/tmp` | tmpfs | Scratch (cleared on reboot) |

On boot, two systemd services run in order:

 1. `nifty-filter-init` seeds the default env file if
    `/var/nifty-filter/router.env` does not exist.
 2. `nifty-filter` runs the binary against the env file and pipes the
    generated nftables ruleset to `nft -f -`.

If the env file is missing or invalid, an emergency lockdown ruleset
is applied that drops all traffic.

### Included services

The base system includes:

 * **nifty-filter** - nftables firewall with IP forwarding
 * **Kea DHCP4** - DHCP server for LAN clients (192.168.10.100-250)
 * **systemd-resolved** - DNS forwarding (Cloudflare 1.1.1.1)
 * **OpenSSH** - key-only auth, no root login

### Use the module in your own NixOS config

You can use the nifty-filter NixOS module in any flake-based system:

```nix
# flake.nix
{
  inputs.nifty-filter.url = "github:EnigmaCurry/nifty-filter/nixos";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs, nifty-filter }: {
    nixosConfigurations.my-router = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        nifty-filter.nixosModules.default
        {
          services.nifty-filter.enable = true;
          # ... rest of your system config
        }
      ];
    };
  };
}
```

The module provides:

 * `services.nifty-filter.enable` - enable the firewall service
 * `services.nifty-filter.configPath` - path to the env file
   (default: `/var/nifty-filter/router.env`)

## Standalone usage

nifty-filter can also be used as a standalone binary on any Linux
system to generate nftables rules.

### Install

[Download the latest release for your platform.](https://github.com/EnigmaCurry/nifty-filter/releases)

Or install via cargo ([crates.io/crates/nifty-filter](https://crates.io/crates/nifty-filter)):

```
cargo install nifty-filter
```

### Examples

There are several included [examples](examples):

 * [home_router.sh](examples/home_router.sh) - Self-contained bash
   script with config defined as environment variables.

 * [home_router.env](examples/home_router.env) - Dot env file with
   all config variables. Pass it to
   `nifty-filter nftables --env-file home_router.env --strict-env`.

### Config styles

Supply configuration via environment variables and/or a `.env` file:

```bash
# Only use the env file:
nifty-filter nftables --env-file .env --strict-env

# Mix env file with shell environment:
INTERFACE_LAN=eth0 INTERFACE_WAN=eth1 nifty-filter nftables --env-file .env

# Environment variables only:
INTERFACE_LAN=eth0 INTERFACE_WAN=eth1 SUBNET_LAN=192.168.10.1/24 \
  nifty-filter nftables
```

Validate the generated output against `nft -c`:

```bash
nifty-filter nftables --env-file .env --strict-env --validate
```
