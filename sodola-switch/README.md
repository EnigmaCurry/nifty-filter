# sodola-switch

CLI and library for managing a Sodola SL-SWTGW218AS 2.5G managed Ethernet switch (8x 2.5GbE RJ45 + 1x 10G SFP+).

The switch has no documented API. This crate reverse-engineers the CGI-based web interface to provide programmatic access.

## Provisioning a switch

The intended workflow: factory reset, configure VLANs and PVIDs, save to flash, then download a backup. The backup can be restored later to return to this exact state.

```bash
# Build
cargo build -p sodola-switch

# On the router, add a route to the switch and log in
sodola-switch route-up
sodola-switch login

# Start clean
sodola-switch factory-reset
# (wait ~15s for reboot)
sodola-switch login

#                                       Ports: 1 2 3 4 5 6 7 8 9
sodola-switch set-vlan 10 --ports U,X,X,X,X,X,X,X,T --name trusted
sodola-switch set-vlan 20 --ports X,U,X,X,X,X,X,X,T --name IoT
sodola-switch set-vlan 30 --ports X,X,U,U,X,X,X,X,T --name guest
sodola-switch set-vlan 40 --ports X,X,X,X,U,U,U,X,T --name lab

sodola-switch set-pvid --ports 1       --pvid 10 --accept untag-only
sodola-switch set-pvid --ports 2       --pvid 20 --accept untag-only
sodola-switch set-pvid --ports 3,4     --pvid 30 --accept untag-only
sodola-switch set-pvid --ports 5,6,7   --pvid 40 --accept untag-only
sodola-switch set-pvid --ports 9       --pvid 1  --accept all

# Persist and download backup
sodola-switch save
sodola-switch backup -o switch_cfg.bin

# Verify
sodola-switch vlans
sodola-switch pvid
sodola-switch json > switch_state.json
```

## Restoring from backup

```bash
sodola-switch login
sodola-switch restore -i switch_cfg.bin
sodola-switch reboot
# (wait ~15s)
```

## Commands

### Connectivity

| Command | Description |
|---------|-------------|
| `login [--user admin] [--password PWD]` | Log in and save credentials. Prompts for password if omitted. |
| `logout` | Invalidate session on switch and remove saved credentials. |
| `route-up [--iface trunk] [--ip 192.168.2.2/24]` | Add IP to trunk interface so the switch is reachable (requires sudo). |
| `route-down [--iface trunk] [--ip 192.168.2.2/24]` | Remove IP from trunk interface (requires sudo). |

### Read-only

| Command | Description |
|---------|-------------|
| `info` | System info: device type, MAC, IP, netmask, gateway, firmware/hardware version. |
| `status` | Port link status (up/down) from the front panel view. |
| `stats` | Port statistics: enable/disable state, link status, TX/RX good/bad packet counters. |
| `vlans` | 802.1Q VLAN table: VID, name, member/tagged/untagged ports. |
| `pvid` | Per-port PVID and accepted frame type settings. |
| `json` | Dump all switch state (info + stats + vlans + pvid) as JSON. |
| `backup [-o FILE]` | Download switch configuration backup (default: `switch_cfg.bin`). |

### Configuration

| Command | Description |
|---------|-------------|
| `set-vlan VID --ports U,T,X,... [--name NAME]` | Create or modify a VLAN. 9 comma-separated port modes: `U`=untagged, `T`=tagged, `X`=not-member. |
| `delete-vlan VID [VID...]` | Delete one or more VLANs by ID. |
| `set-pvid --ports 1,2 --pvid 10 [--accept untag-only]` | Set PVID and accepted frame type for ports. Accept values: `all`, `tag-only`, `untag-only`. |
| `save` | Save running configuration to flash ROM. |
| `restore -i FILE` | Upload a configuration backup (reboot required to apply). |
| `reboot` | Reboot the switch (~15 seconds). |
| `factory-reset` | Restore factory defaults (wipes config and reboots). |

## Global options

| Option | Env var | Default | Description |
|--------|---------|---------|-------------|
| `--url` | `SODOLA_URL` | `http://192.168.2.1` | Switch base URL |

Credentials are stored at `~/.local/share/nifty-filter/sodola-switch/credentials` (mode 0600).

Route defaults can be overridden with `SODOLA_MGMT_IFACE` and `SODOLA_ROUTER_IP` env vars.

## How it works

The switch uses a CGI web interface with cookie-based authentication. The auth token is `md5(username + password)`, sent as the `Cookie` header. This crate:

1. Computes the MD5 token and POSTs to `/login.cgi` to establish a session
2. Sends the cookie with each subsequent request
3. Parses the HTML responses (simple string matching, no heavy HTML parser)
4. Detects session expiry via JS redirect to `/login.cgi` in responses

**Note:** The switch firmware requires HTTP headers with standard casing (`Cookie:` not `cookie:`). This is why we use ureq 2 instead of reqwest or ureq 3, which lowercase all header names via the `http` crate.

## Library usage

The `SodolaClient` struct can be used as a library:

```rust
use sodola_switch::SodolaClient;

let mut client = SodolaClient::new("http://192.168.2.1");
client.login("admin", "admin").unwrap();

let info = client.info().unwrap();
println!("{}", info);

let vlans = client.vlans().unwrap();
for v in &vlans {
    println!("VLAN {}: {}", v.vid, v.name);
}

client.logout().unwrap();
```
