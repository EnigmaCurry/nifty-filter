use askama::Template;
use clap::{Parser, Subcommand};
use env_logger;
use log::{error, info};
use std::collections::{HashMap, HashSet};
use std::env;
use std::process::exit;
#[cfg(feature = "nixos")]
mod config;
mod format;
pub mod generate;
pub mod hcl_config;
#[cfg(feature = "nixos")]
mod install;
mod parsers;
#[cfg(feature = "nixos")]
mod pve_setup;
pub mod qos;
pub mod vlan;
use hcl_config::{parse_hcl, HclConfig};
use parsers::*;
use qos::{QosConfig, QosOverride};
use vlan::Vlan;
#[allow(unused_imports)]
use std::net::IpAddr;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "RouterConfig")]
#[command(about = "Generates router configuration from HCL config file")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive configuration menu
    #[cfg(feature = "nixos")]
    Config,

    /// Install nifty-filter to disk from the live ISO
    #[cfg(feature = "nixos")]
    Install {
        /// Set a git remote for config updates
        #[arg(long)]
        git_remote: Option<String>,
    },

    /// Reboot into maintenance mode (read-write root)
    #[cfg(feature = "nixos")]
    Maintenance,

    /// Upgrade the system in place
    #[cfg(feature = "nixos")]
    Upgrade {
        /// Target branch (overrides saved branch)
        branch: Option<String>,
    },

    /// Interactive PVE VM setup wizard (outputs shell variables)
    #[cfg(feature = "nixos")]
    PveSetup {
        /// PVE host to connect to
        pve_host: String,
    },

    /// Print version and build info
    Version,

    /// Generate QoS (tc/CAKE) shell commands for WAN traffic shaping
    Qos {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
    },

    /// Generate nftables configuration
    #[command(alias = "nft")]
    Nftables {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,

        /// Validate with nft -c (only works if interfaces exist on this host)
        #[arg(long)]
        validate: bool,

        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Print hostname from config (or "nifty-filter" if not set)
    Hostname {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
    },

    /// Print a config value by key (wan-name, trunk-name, mgmt-name, mgmt-subnet, enable-ipv6, dashboard-port, iperf-port)
    Get {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
        /// Key to retrieve
        key: String,
    },

    /// Generate system configuration files from HCL config
    Generate {
        #[command(subcommand)]
        what: GenerateCommands,
    },
}

#[derive(Subcommand)]
enum GenerateCommands {
    /// Generate systemd .link files for interface renaming by MAC address
    Linkfiles {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
        /// Output directory for .link files
        #[arg(long, short)]
        output_dir: String,
    },
    /// Generate systemd-networkd .network and .netdev files
    Networkd {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
        /// Output directory for network files
        #[arg(long, short)]
        output_dir: String,
    },
    /// Generate dnsmasq.conf
    Dnsmasq {
        /// Path to the HCL config file
        #[arg(long, short)]
        config: String,
        /// Output file path
        #[arg(long, short = 'O')]
        output: String,
    },
    /// Generate minimal DNS-only dnsmasq.conf (no HCL config needed)
    DnsmasqMinimal {
        /// Output file path
        #[arg(long, short = 'O')]
        output: String,
    },
}

#[derive(Template)]
#[template(path = "router.nft.txt")]
struct RouterTemplate {
    interface_trunk: Interface,
    interface_wan: Interface,
    interface_mgmt: String,
    subnet_mgmt_ipv4: String,

    // Protocol enablement
    enable_ipv4: bool,
    enable_ipv6: bool,

    // VLAN configuration
    vlan_aware_switch: bool,
    vlans: Vec<Vlan>,

    // WAN-side ICMP
    icmp_accept_wan: String,
    icmpv6_accept_wan: String,

    // WAN-side port accepts
    tcp_accept_wan: String,
    udp_accept_wan: String,

    // WAN-side forward routes
    tcp_forward_wan: ForwardRouteList,
    udp_forward_wan: ForwardRouteList,

    // iperf3 server port
    iperf_port: u16,

    // Anti-spoofing: bogon source addresses to drop on WAN
    wan_bogons_ipv4: String,
    wan_bogons_ipv6: String,

    // QoS: DSCP marking for upload traffic prioritization
    qos_enabled: bool,
    qos_overrides: Vec<QosOverride>,
}

impl RouterTemplate {
    fn from_hcl(config: &HclConfig) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();

        // Interfaces
        let interface_trunk = Interface::new(&config.interfaces.trunk_name())
            .unwrap_or_else(|e| { errors.push(e); Interface::new("eth0").unwrap() });
        let interface_wan = Interface::new(&config.interfaces.wan_name())
            .unwrap_or_else(|e| { errors.push(e); Interface::new("eth0").unwrap() });
        let interface_mgmt = config.interfaces.mgmt_name().unwrap_or("").to_string();
        let subnet_mgmt_ipv4 = if !interface_mgmt.is_empty() {
            match config.interfaces.mgmt_subnet() {
                Some(s) => match Subnet::new(s) {
                    Ok(subnet) => subnet.to_string(),
                    Err(e) => { errors.push(e); String::new() }
                },
                None => {
                    errors.push("interfaces.mgmt.subnet is required when interfaces.mgmt is set.".to_string());
                    String::new()
                }
            }
        } else {
            String::new()
        };

        // Protocol enablement
        let enable_ipv4 = config.wan.enable_ipv4;
        let enable_ipv6 = config.wan.enable_ipv6;
        if !enable_ipv4 && !enable_ipv6 {
            errors.push("At least one of wan.enable_ipv4 or wan.enable_ipv6 must be true.".to_string());
        }

        let vlan_aware_switch = config.vlan_aware_switch;

        // WAN ICMP
        let icmp_accept_wan = if enable_ipv4 {
            let types: Vec<IcmpType> = config.wan.icmp_accept.iter()
                .filter_map(|s| IcmpType::new(s).map_err(|e| errors.push(format!("wan.icmp_accept: {}", e))).ok())
                .collect();
            IcmpType::vec_to_string(&types)
        } else {
            String::new()
        };

        let icmpv6_accept_wan = if enable_ipv6 {
            if config.wan.icmpv6_accept.is_empty() {
                // Defaults required for IPv6 to function (ND, error types)
                Icmpv6Type::vec_to_string(&[
                    Icmpv6Type::NdNeighborSolicit,
                    Icmpv6Type::NdNeighborAdvert,
                    Icmpv6Type::NdRouterSolicit,
                    Icmpv6Type::NdRouterAdvert,
                    Icmpv6Type::DestinationUnreachable,
                    Icmpv6Type::PacketTooBig,
                    Icmpv6Type::TimeExceeded,
                ])
            } else {
                let types: Vec<Icmpv6Type> = config.wan.icmpv6_accept.iter()
                    .filter_map(|s| Icmpv6Type::new(s).map_err(|e| errors.push(format!("wan.icmpv6_accept: {}", e))).ok())
                    .collect();
                Icmpv6Type::vec_to_string(&types)
            }
        } else {
            String::new()
        };

        // WAN ports
        let tcp_accept_wan = config.wan.tcp_accept.iter()
            .map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
        let udp_accept_wan = config.wan.udp_accept.iter()
            .map(|p| p.to_string()).collect::<Vec<_>>().join(", ");

        // WAN forwards
        let tcp_forward_wan = ForwardRouteList::new(&config.wan.tcp_forward.join(", "))
            .unwrap_or_else(|e| { errors.push(format!("wan.tcp_forward: {}", e)); ForwardRouteList::new("").unwrap() });
        let udp_forward_wan = ForwardRouteList::new(&config.wan.udp_forward.join(", "))
            .unwrap_or_else(|e| { errors.push(format!("wan.udp_forward: {}", e)); ForwardRouteList::new("").unwrap() });

        let iperf_port = config.iperf_port.unwrap_or(5201);

        // Bogons (hardcoded defaults)
        let wan_bogons_ipv4 = if enable_ipv4 {
            CidrList::new("0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24, 224.0.0.0/4, 240.0.0.0/4").unwrap().to_string()
        } else {
            String::new()
        };
        let wan_bogons_ipv6 = if enable_ipv6 {
            CidrList::new("::/128, ::1/128, fc00::/7, ff00::/8").unwrap().to_string()
        } else {
            String::new()
        };

        // QoS
        let (qos_enabled, qos_overrides) = if let Some(qos) = &config.qos {
            if qos.upload_mbps == 0 {
                errors.push("qos.upload_mbps must be greater than 0.".to_string());
            }
            if qos.download_mbps == 0 {
                errors.push("qos.download_mbps must be greater than 0.".to_string());
            }
            if qos.shave_percent >= 100 {
                errors.push("qos.shave_percent must be less than 100.".to_string());
            }

            let mut overrides = Vec::new();
            if let Some(ovr) = &qos.overrides {
                for (class, cidrs) in [
                    (qos::QosClass::Voice, &ovr.voice),
                    (qos::QosClass::Video, &ovr.video),
                    (qos::QosClass::Besteffort, &ovr.besteffort),
                    (qos::QosClass::Bulk, &ovr.bulk),
                ] {
                    if !cidrs.is_empty() {
                        match CidrList::new(&cidrs.join(", ")) {
                            Ok(list) => {
                                let o = QosOverride::from_cidr_list(class, &list);
                                if !o.cidrs_ipv4.is_empty() || !o.cidrs_ipv6.is_empty() {
                                    overrides.push(o);
                                }
                            }
                            Err(e) => errors.push(format!("qos.overrides: {}", e)),
                        }
                    }
                }
            }
            (true, overrides)
        } else {
            (false, Vec::new())
        };

        // VLANs
        let vlans = Self::convert_vlans(config, enable_ipv4, &mut errors);

        // Validate: bandwidth requires qos block
        if !qos_enabled {
            for (name, vhcl) in &config.vlan {
                if vhcl.bandwidth.is_some() {
                    errors.push(format!("vlan \"{}\".bandwidth requires a qos block to be configured.", name));
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(RouterTemplate {
            interface_trunk,
            interface_wan,
            interface_mgmt,
            subnet_mgmt_ipv4,
            enable_ipv4,
            enable_ipv6,
            vlan_aware_switch,
            vlans,
            icmp_accept_wan,
            icmpv6_accept_wan,
            tcp_accept_wan,
            udp_accept_wan,
            tcp_forward_wan,
            udp_forward_wan,
            iperf_port,
            wan_bogons_ipv4,
            wan_bogons_ipv6,
            qos_enabled,
            qos_overrides,
        })
    }

    fn convert_vlans(
        config: &HclConfig,
        enable_ipv4: bool,
        errors: &mut Vec<String>,
    ) -> Vec<Vlan> {
        let trunk_name = &config.interfaces.trunk_name();

        // Sort by ID for deterministic output
        let mut entries: Vec<(&String, &hcl_config::VlanHclConfig)> = config.vlan.iter().collect();
        entries.sort_by_key(|(_, v)| v.id);

        if entries.is_empty() {
            errors.push("At least one vlan block must be configured.".to_string());
        }

        // Validate VLAN IDs
        let mut seen_ids = HashSet::new();
        for (name, v) in &entries {
            if v.id == 0 || v.id > 4094 {
                errors.push(format!("vlan \"{}\": VLAN ID {} out of range (1-4094).", name, v.id));
            }
            if !seen_ids.insert(v.id) {
                errors.push(format!("vlan \"{}\": duplicate VLAN ID {}.", name, v.id));
            }
            if config.vlan_aware_switch && v.id == 1 {
                errors.push(format!(
                    "vlan \"{}\": VLAN ID 1 is not allowed when vlan_aware_switch is true. All VLANs must have ID > 1.",
                    name
                ));
            }
        }

        // Build name -> (id, interface_name) lookup for inter-VLAN rules
        let name_lookup: HashMap<&str, (u16, String)> = entries.iter().map(|(name, v)| {
            let iface = if v.id == 1 && !config.vlan_aware_switch {
                trunk_name.to_string()
            } else {
                name.to_string()
            };
            (name.as_str(), (v.id, iface))
        }).collect();

        let mut vlans: Vec<Vlan> = Vec::new();

        for (name, vhcl) in &entries {
            let (_, ref interface_name) = name_lookup[name.as_str()];

            // IPv4
            let subnet_ipv4 = vhcl.ipv4.as_ref().map(|v| v.subnet.clone()).unwrap_or_default();
            if enable_ipv4 && subnet_ipv4.is_empty() {
                errors.push(format!("vlan \"{}\".ipv4.subnet is required when wan.enable_ipv4 is true.", name));
            }
            let egress_allowed_ipv4 = match &vhcl.ipv4 {
                Some(ipv4) if !ipv4.egress.is_empty() => {
                    let joined = ipv4.egress.join(", ");
                    match CidrList::new(&joined) {
                        Ok(list) => list.to_string(),
                        Err(e) => { errors.push(format!("vlan \"{}\".ipv4.egress: {}", name, e)); String::new() }
                    }
                }
                _ => String::new(),
            };

            // IPv6
            let subnet_ipv6 = vhcl.ipv6.as_ref().map(|v| v.subnet.clone()).unwrap_or_default();
            let egress_allowed_ipv6 = match &vhcl.ipv6 {
                Some(ipv6) if !ipv6.egress.is_empty() => {
                    let joined = ipv6.egress.join(", ");
                    match CidrList::new(&joined) {
                        Ok(list) => list.to_string(),
                        Err(e) => { errors.push(format!("vlan \"{}\".ipv6.egress: {}", name, e)); String::new() }
                    }
                }
                _ => String::new(),
            };

            // Firewall
            let (icmp_accept, icmpv6_accept, tcp_accept, udp_accept) = match &vhcl.firewall {
                Some(fw) => {
                    let icmp = if enable_ipv4 {
                        let types: Vec<IcmpType> = fw.icmp_accept.iter()
                            .filter_map(|s| IcmpType::new(s).map_err(|e| errors.push(format!("vlan \"{}\".firewall: {}", name, e))).ok())
                            .collect();
                        IcmpType::vec_to_string(&types)
                    } else {
                        String::new()
                    };

                    let icmpv6 = if !subnet_ipv6.is_empty() {
                        let types: Vec<Icmpv6Type> = fw.icmpv6_accept.iter()
                            .filter_map(|s| Icmpv6Type::new(s).map_err(|e| errors.push(format!("vlan \"{}\".firewall: {}", name, e))).ok())
                            .collect();
                        Icmpv6Type::vec_to_string(&types)
                    } else {
                        String::new()
                    };

                    let tcp = fw.tcp_accept.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                    let udp = fw.udp_accept.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");

                    (icmp, icmpv6, tcp, udp)
                }
                None => (String::new(), String::new(), String::new(), String::new()),
            };

            // QoS class
            let qos_class = vhcl.qos_class.as_ref().map(|s| {
                qos::QosClass::new(s).unwrap_or_else(|e| {
                    errors.push(format!("vlan \"{}\".qos_class: {}", name, e));
                    qos::QosClass::Besteffort
                })
            });

            // Per-VLAN bandwidth limits
            let (bandwidth_upload_kbit, bandwidth_download_kbit) = if let Some(bw) = &vhcl.bandwidth {
                if bw.upload_mbps.is_none() && bw.download_mbps.is_none() {
                    errors.push(format!("vlan \"{}\".bandwidth: at least one of upload_mbps or download_mbps must be set.", name));
                }
                if let Some(up) = bw.upload_mbps {
                    if up == 0 {
                        errors.push(format!("vlan \"{}\".bandwidth.upload_mbps must be greater than 0.", name));
                    }
                }
                if let Some(down) = bw.download_mbps {
                    if down == 0 {
                        errors.push(format!("vlan \"{}\".bandwidth.download_mbps must be greater than 0.", name));
                    }
                }
                (bw.upload_mbps.map(|v| v * 1000), bw.download_mbps.map(|v| v * 1000))
            } else {
                (None, None)
            };

            // DHCP
            let dhcp_enabled = vhcl.dhcp.is_some();
            let (dhcp_pool_start, dhcp_pool_end, dhcp_router, dhcp_dns) = vhcl.dhcp.as_ref()
                .map(|d| (d.pool_start.clone(), d.pool_end.clone(), d.router.clone(), d.dns.clone()))
                .unwrap_or_default();

            // DHCPv6
            let dhcpv6_enabled = vhcl.dhcpv6.is_some();
            let (dhcpv6_pool_start, dhcpv6_pool_end) = vhcl.dhcpv6.as_ref()
                .map(|d| (d.pool_start.clone(), d.pool_end.clone()))
                .unwrap_or_default();

            // Forward routes
            let tcp_forward = ForwardRouteList::new(&vhcl.tcp_forward.join(", "))
                .unwrap_or_else(|e| { errors.push(format!("vlan \"{}\".tcp_forward: {}", name, e)); ForwardRouteList::new("").unwrap() });
            let udp_forward = ForwardRouteList::new(&vhcl.udp_forward.join(", "))
                .unwrap_or_else(|e| { errors.push(format!("vlan \"{}\".udp_forward: {}", name, e)); ForwardRouteList::new("").unwrap() });

            // Inbound rules
            let tcp_allow_inbound = InboundRuleList::new(&vhcl.allow_inbound_tcp.join(", "))
                .unwrap_or_else(|e| { errors.push(format!("vlan \"{}\".allow_inbound_tcp: {}", name, e)); InboundRuleList::new("").unwrap() });
            let udp_allow_inbound = InboundRuleList::new(&vhcl.allow_inbound_udp.join(", "))
                .unwrap_or_else(|e| { errors.push(format!("vlan \"{}\".allow_inbound_udp: {}", name, e)); InboundRuleList::new("").unwrap() });

            vlans.push(Vlan {
                id: vhcl.id,
                name: name.to_string(),
                interface_name: interface_name.clone(),
                subnet_ipv4,
                subnet_ipv6,
                egress_allowed_ipv4,
                egress_allowed_ipv6,
                icmp_accept,
                icmpv6_accept,
                tcp_accept,
                udp_accept,
                tcp_forward,
                udp_forward,
                tcp_allow_inbound,
                udp_allow_inbound,
                tcp_allow_inter_vlan: InterVlanRuleList::new(),
                udp_allow_inter_vlan: InterVlanRuleList::new(),
                qos_class,
                bandwidth_upload_kbit,
                bandwidth_download_kbit,
                iperf_enabled: vhcl.iperf_enabled,
                dhcp_enabled,
                dhcp_pool_start,
                dhcp_pool_end,
                dhcp_router,
                dhcp_dns,
                dhcpv6_enabled,
                dhcpv6_pool_start,
                dhcpv6_pool_end,
            });
        }

        // Process inter-VLAN rules (needs all VLANs built first for name lookup)
        for (name, vhcl) in &entries {
            for (src_name, rules) in &vhcl.allow_from {
                if let Some(&(src_id, ref src_iface)) = name_lookup.get(src_name.as_str()) {
                    if let Some(target) = vlans.iter_mut().find(|v| v.name == **name) {
                        let joined_tcp = rules.tcp.join(", ");
                        if !joined_tcp.is_empty() {
                            if let Err(e) = target.tcp_allow_inter_vlan.add_entry(src_id, src_iface.clone(), &joined_tcp) {
                                errors.push(format!("vlan \"{}\".allow_from \"{}\".tcp: {}", name, src_name, e));
                            }
                        }
                        let joined_udp = rules.udp.join(", ");
                        if !joined_udp.is_empty() {
                            if let Err(e) = target.udp_allow_inter_vlan.add_entry(src_id, src_iface.clone(), &joined_udp) {
                                errors.push(format!("vlan \"{}\".allow_from \"{}\".udp: {}", name, src_name, e));
                            }
                        }
                    }
                } else {
                    errors.push(format!("vlan \"{}\".allow_from \"{}\": unknown source VLAN name.", name, src_name));
                }
            }
        }

        vlans
    }
}

#[derive(Template)]
#[template(path = "qos.sh.txt")]
struct QosTemplate {
    interface_wan: Interface,
    upload_kbit: u32,
    download_kbit: u32,
    vlan_upload_limits: Vec<qos::QosVlanBandwidth>,
    vlan_download_limits: Vec<qos::QosVlanBandwidth>,
    default_upload_kbit: u32,
    default_download_kbit: u32,
}

pub fn validate_nftables_config(config: &str) -> Result<(), String> {
    let output = Command::new("nft")
        .arg("-c")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                stdin.write_all(config.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("Failed to run nft command: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Read and parse an HCL config file, exiting on error.
fn load_hcl_config(path: &str) -> HclConfig {
    let contents = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error: Failed to read {}: {}", path, e);
        exit(1);
    });
    parse_hcl(&contents).unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        exit(1);
    })
}

#[cfg(feature = "nixos")]
fn run_maintenance() {
    use std::process::{exit, Command};

    let script = include_str!("../nix/nifty-maintenance.sh");
    let tmp = std::env::temp_dir().join("nifty-maintenance.sh");
    std::fs::write(&tmp, script).expect("failed to write temp file");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .expect("failed to chmod temp file");

    let status = Command::new(tmp)
        .status()
        .expect("failed to execute maintenance script");
    exit(status.code().unwrap_or(1));
}

#[cfg(feature = "nixos")]
fn run_upgrade(branch: Option<String>) {
    use std::process::{exit, Command};

    let script = include_str!("../nix/nifty-upgrade.sh");
    let tmp = std::env::temp_dir().join("nifty-upgrade.sh");
    std::fs::write(&tmp, script).expect("failed to write temp file");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .expect("failed to chmod temp file");

    let mut cmd = Command::new(&tmp);
    cmd.env("TMPDIR", "/var/tmp");
    if let Some(b) = branch {
        cmd.arg(b);
    }
    let status = cmd.status().expect("failed to execute upgrade script");
    exit(status.code().unwrap_or(1));
}

fn app() {
    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "nixos")]
        Commands::Config => config::run(),
        #[cfg(feature = "nixos")]
        Commands::Install { git_remote } => install::run(git_remote),
        #[cfg(feature = "nixos")]
        Commands::Maintenance => run_maintenance(),
        #[cfg(feature = "nixos")]
        Commands::Upgrade { branch } => run_upgrade(branch),
        #[cfg(feature = "nixos")]
        Commands::PveSetup { pve_host } => pve_setup::run(&pve_host),
        Commands::Version => {
            println!("nifty-filter {} ({})", env!("CARGO_PKG_VERSION"), option_env!("GIT_SHA").unwrap_or("unknown"));
        }
        Commands::Qos { config } => {
            let hcl_config = load_hcl_config(&config);

            match &hcl_config.qos {
                Some(qos_hcl) => {
                    let mut errors = Vec::new();
                    let interface_wan = Interface::new(&hcl_config.interfaces.wan_name())
                        .unwrap_or_else(|e| { errors.push(e); Interface::new("eth0").unwrap() });

                    match QosConfig::from_hcl(qos_hcl) {
                        Ok(qos_config) => {
                            // Collect per-VLAN bandwidth limits
                            let mut vlan_upload_limits = Vec::new();
                            let mut vlan_download_limits = Vec::new();
                            let mut entries: Vec<_> = hcl_config.vlan.iter().collect();
                            entries.sort_by_key(|(_, v)| v.id);
                            for (name, vhcl) in &entries {
                                if let Some(bw) = &vhcl.bandwidth {
                                    if let Some(up) = bw.upload_mbps {
                                        if up == 0 {
                                            errors.push(format!("vlan \"{}\".bandwidth.upload_mbps must be greater than 0.", name));
                                        } else {
                                            vlan_upload_limits.push(qos::QosVlanBandwidth {
                                                vlan_id: vhcl.id,
                                                kbit: up * 1000,
                                            });
                                        }
                                    }
                                    if let Some(down) = bw.download_mbps {
                                        if down == 0 {
                                            errors.push(format!("vlan \"{}\".bandwidth.download_mbps must be greater than 0.", name));
                                        } else {
                                            vlan_download_limits.push(qos::QosVlanBandwidth {
                                                vlan_id: vhcl.id,
                                                kbit: down * 1000,
                                            });
                                        }
                                    }
                                }
                            }

                            let upload_bw_sum: u32 = vlan_upload_limits.iter().map(|v| v.kbit).sum();
                            let default_upload_kbit = if !vlan_upload_limits.is_empty() && upload_bw_sum >= qos_config.upload_kbit {
                                errors.push("Sum of per-VLAN bandwidth.upload_mbps exceeds total qos.upload_mbps (after shave).".to_string());
                                0
                            } else if vlan_upload_limits.is_empty() {
                                qos_config.upload_kbit
                            } else {
                                qos_config.upload_kbit - upload_bw_sum
                            };

                            let download_bw_sum: u32 = vlan_download_limits.iter().map(|v| v.kbit).sum();
                            let default_download_kbit = if !vlan_download_limits.is_empty() && download_bw_sum >= qos_config.download_kbit {
                                errors.push("Sum of per-VLAN bandwidth.download_mbps exceeds total qos.download_mbps (after shave).".to_string());
                                0
                            } else if vlan_download_limits.is_empty() {
                                qos_config.download_kbit
                            } else {
                                qos_config.download_kbit - download_bw_sum
                            };

                            if !errors.is_empty() {
                                for err in errors {
                                    eprintln!("Error: {}", err);
                                }
                                exit(1);
                            }
                            let tmpl = QosTemplate {
                                interface_wan,
                                upload_kbit: qos_config.upload_kbit,
                                download_kbit: qos_config.download_kbit,
                                vlan_upload_limits,
                                vlan_download_limits,
                                default_upload_kbit,
                                default_download_kbit,
                            };
                            println!("{}", tmpl.render().unwrap());
                        }
                        Err(qos_errors) => {
                            for err in qos_errors {
                                eprintln!("Error: {}", err);
                            }
                            exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("QoS not configured (no qos block in config), skipping.");
                }
            }
        }
        Commands::Nftables {
            config,
            validate,
            verbose,
        } => {
            if verbose {
                env::set_var("RUST_LOG", "info");
            }
            env_logger::init();

            let hcl_config = load_hcl_config(&config);
            info!("Loaded configuration from file: {}", config);

            match RouterTemplate::from_hcl(&hcl_config) {
                Ok(router) => {
                    let text = format::reduce_blank_lines(&router.render().unwrap());
                    if validate {
                        match validate_nftables_config(&text) {
                            Ok(_valid) => {}
                            Err(e) => {
                                error!("Error validating nftables config: {}", e);
                                exit(1)
                            }
                        }
                    }
                    println!("{}", text)
                }
                Err(errors) => {
                    for err in errors {
                        eprintln!("Error: {}", err);
                    }
                    std::process::exit(1);
                }
            }
        }
        Commands::Hostname { config } => {
            let hcl_config = load_hcl_config(&config);
            println!(
                "{}",
                hcl_config.hostname.as_deref().unwrap_or("nifty-filter")
            );
        }
        Commands::Get { config, key } => {
            let hcl_config = load_hcl_config(&config);
            let value = match key.as_str() {
                "wan-name" => Some(hcl_config.interfaces.wan_name().to_string()),
                "trunk-name" => Some(hcl_config.interfaces.trunk_name().to_string()),
                "mgmt-name" => hcl_config.interfaces.mgmt_name().map(|s| s.to_string()),
                "mgmt-subnet" => hcl_config.interfaces.mgmt_subnet().map(|s| s.to_string()),
                "enable-ipv6" => Some(hcl_config.wan.enable_ipv6.to_string()),
                "dashboard-port" => Some(hcl_config.dashboard_port.unwrap_or(3000).to_string()),
                "iperf-port" => Some(hcl_config.iperf_port.unwrap_or(5201).to_string()),
                "switch-router-ip" => hcl_config.switch.as_ref().and_then(|s| s.router_ip.clone()),
                _ => {
                    eprintln!("Unknown key: {}", key);
                    exit(1);
                }
            };
            if let Some(v) = value {
                println!("{}", v);
            }
        }
        Commands::Generate { what } => match what {
            GenerateCommands::Linkfiles { config, output_dir } => {
                let hcl_config = load_hcl_config(&config);
                if let Err(e) = generate::generate_linkfiles(&hcl_config, &output_dir) {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
            }
            GenerateCommands::Networkd { config, output_dir } => {
                let hcl_config = load_hcl_config(&config);
                if let Err(e) = generate::generate_networkd(&hcl_config, &output_dir) {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
            }
            GenerateCommands::Dnsmasq { config, output } => {
                let hcl_config = load_hcl_config(&config);
                if let Err(e) = generate::generate_dnsmasq(&hcl_config, &output) {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
            }
            GenerateCommands::DnsmasqMinimal { output } => {
                if let Err(e) = generate::generate_dnsmasq_minimal(&output) {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
            }
        },
    }
}

fn main() {
    app()
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[test]
    fn test_forward_route_parsing() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(route_list.get_routes().len(), 2);
        assert_eq!(route_list.get_routes()[0].incoming_port, 8080);
        assert_eq!(
            route_list.get_routes()[0].destination_ip,
            "192.168.1.100".parse::<IpAddr>().unwrap()
        );
        assert_eq!(route_list.get_routes()[0].destination_port, 80);
    }

    #[test]
    fn test_forward_route_invalid() {
        let input = "8080:192.168.1.100, 8443:192.168.1.101:443";
        assert!(ForwardRouteList::new(input).is_err());
    }

    #[test]
    fn test_to_string() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(
            route_list.to_string(),
            "8080:192.168.1.100:80, 8443:192.168.1.101:443"
        );
    }

    #[test]
    fn test_simple_mode_vlan1() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan "lan" {
                id = 1
                ipv4 {
                    subnet = "192.168.10.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        // VLAN 1 uses bare trunk interface
        assert!(rendered.contains(r#"iif "trunk""#));
        assert!(!rendered.contains("trunk.1"));
        // Egress rule
        assert!(rendered.contains("ip saddr 192.168.10.1/24 ip daddr { 0.0.0.0/0 }"));
        // SSH accept
        assert!(rendered.contains("tcp dport { 22 }"));
        // Not switch-aware
        assert!(!rendered.contains("Dropped untagged trunk"));
        assert!(!rendered.contains("inter-VLAN"));
    }

    #[test]
    fn test_vlan_aware_multi_vlan() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.10.0.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                firewall {
                    tcp_accept = []
                    udp_accept = [67, 68]
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        // VLAN sub-interfaces use names from HCL keys
        assert!(rendered.contains(r#"iif "trusted""#));
        assert!(rendered.contains(r#"iif "iot""#));
        // Trusted has egress
        assert!(rendered.contains("ip saddr 10.10.0.1/24 ip daddr { 0.0.0.0/0 }"));
        // IoT has no egress
        assert!(!rendered.contains("ip saddr 10.20.0.1/24 ip daddr"));
        // Per-VLAN input chains
        assert!(rendered.contains(r#"iif "trusted" jump input_vlan_10"#));
        assert!(rendered.contains(r#"iif "iot" jump input_vlan_20"#));
        // SSH in trusted chain
        assert!(rendered.contains(r#"ip saddr 10.10.0.1/24 tcp dport { 22 }"#));
        // Switch-aware: untagged trunk drop
        assert!(rendered.contains("Dropped untagged trunk input"));
        assert!(rendered.contains("Dropped untagged trunk forward"));
    }

    #[test]
    fn test_vlan_aware_rejects_vlan_1() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            vlan "lan" {
                id = 1
                ipv4 { subnet = "192.168.1.1/24" }
            }
            vlan "trusted" {
                id = 10
                ipv4 { subnet = "10.10.0.1/24" }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let result = RouterTemplate::from_hcl(&config);
        let errors = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(errors.iter().any(|e| e.contains("VLAN ID 1") && e.contains("not allowed")));
    }

    #[test]
    fn test_vlan_names_as_interfaces() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.10.0.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                firewall {
                    tcp_accept = []
                    udp_accept = [67, 68]
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        // Custom names from HCL keys
        assert!(rendered.contains(r#"iif "trusted""#));
        assert!(rendered.contains(r#"iif "iot""#));
        assert!(!rendered.contains("trunk.10"));
        assert!(!rendered.contains("trunk.20"));
    }

    #[test]
    fn test_per_vlan_router_access_isolation() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            vlan "trusted" {
                id = 10
                ipv4 { subnet = "10.10.0.1/24" }
                firewall {
                    icmp_accept = ["echo-request", "echo-reply", "destination-unreachable", "time-exceeded"]
                    tcp_accept  = [22]
                    udp_accept  = [67, 68]
                }
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                firewall {
                    icmp_accept = ["destination-unreachable"]
                    tcp_accept  = []
                    udp_accept  = [67, 68]
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        assert!(rendered.contains(r#"iif "trusted" jump input_vlan_10"#));
        assert!(rendered.contains(r#"ip saddr 10.10.0.1/24 tcp dport { 22 }"#));
        assert!(rendered.contains(r#"iif "iot" jump input_vlan_20"#));
        assert!(rendered.contains(r#"ip saddr 10.10.0.1/24 icmp type { echo-request, echo-reply, destination-unreachable, time-exceeded }"#));
        assert!(rendered.contains(r#"ip saddr 10.20.0.1/24 icmp type { destination-unreachable }"#));
    }

    #[test]
    fn test_inter_vlan_allow_rules() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.99.10.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
            }
            vlan "lab" {
                id = 40
                ipv4 { subnet = "10.99.40.1/24" }
                firewall {
                    tcp_accept = []
                    udp_accept = [67, 68]
                }
                allow_from "trusted" {
                    tcp = ["10.99.40.5:80", "10.99.10.50:10.99.40.5:443"]
                    udp = ["10.99.40.5:53"]
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        // 2-tuple: no saddr filter
        assert!(rendered.contains(
            r#"iif "trusted" oif "lab" ip daddr 10.99.40.5 tcp dport 80 accept"#
        ));
        // 3-tuple: saddr filter
        assert!(rendered.contains(
            r#"iif "trusted" oif "lab" ip saddr 10.99.10.50 ip daddr 10.99.40.5 tcp dport 443 accept"#
        ));
        // UDP rule
        assert!(rendered.contains(
            r#"iif "trusted" oif "lab" ip daddr 10.99.40.5 udp dport 53 accept"#
        ));
        // Comments
        assert!(rendered.contains("Allow inter-VLAN TCP from VLAN 10 to VLAN 40"));
        assert!(rendered.contains("Allow inter-VLAN UDP from VLAN 10 to VLAN 40"));
    }

    #[test]
    fn test_qos_disabled_no_mangle_table() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan "lan" {
                id = 1
                ipv4 { subnet = "192.168.10.1/24" }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        assert!(!rendered.contains("table inet mangle"));
        assert!(!rendered.contains("dscp set"));
        assert!(!rendered.contains("flowtable"));
    }

    #[test]
    fn test_qos_enabled_dscp_marking() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            qos {
                upload_mbps = 20
                download_mbps = 300
            }
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.10.0.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
                qos_class = "voice"
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                firewall {
                    tcp_accept = []
                    udp_accept = [67, 68]
                }
                qos_class = "bulk"
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        assert!(rendered.contains("table inet mangle"));
        // VLAN 10 marked as voice (EF)
        assert!(rendered.contains(r#"oif "wan" ip saddr 10.10.0.1/24 ip dscp set ef"#));
        // VLAN 20 marked as bulk (CS1)
        assert!(rendered.contains(r#"oif "wan" ip saddr 10.20.0.1/24 ip dscp set cs1"#));
    }

    #[test]
    fn test_qos_override_rules() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            qos {
                upload_mbps = 20
                download_mbps = 300
                overrides {
                    voice = ["192.168.10.50", "192.168.10.51"]
                    bulk  = ["192.168.10.0/24"]
                }
            }
            vlan "lan" {
                id = 1
                ipv4 { subnet = "192.168.10.1/24" }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        assert!(rendered.contains("192.168.10.50/32, 192.168.10.51/32"));
        assert!(rendered.contains("ip dscp set ef"));
        assert!(rendered.contains("192.168.10.0/24"));
        assert!(rendered.contains("ip dscp set cs1"));
    }

    #[test]
    fn test_qos_no_flowtable() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan "lan" {
                id = 1
                ipv4 { subnet = "192.168.10.1/24" }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        assert!(!rendered.contains("flowtable"));
        assert!(!rendered.contains("flow add @ft"));
    }

    #[test]
    fn test_bandwidth_fwmark_rules() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan_aware_switch = true
            qos {
                upload_mbps = 20
                download_mbps = 300
            }
            vlan "trusted" {
                id = 10
                ipv4 {
                    subnet = "10.10.0.1/24"
                    egress = ["0.0.0.0/0"]
                }
                firewall {
                    tcp_accept = [22]
                    udp_accept = [67, 68]
                }
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                firewall {
                    tcp_accept = []
                    udp_accept = [67, 68]
                }
                bandwidth {
                    upload_mbps = 5
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let tmpl = RouterTemplate::from_hcl(&config).unwrap();
        let rendered = tmpl.render().unwrap();

        // VLAN 20 should get a fwmark rule
        assert!(rendered.contains(r#"oif "wan" ip saddr 10.20.0.1/24 meta mark set 20"#));
        // VLAN 10 should NOT get a fwmark rule (no bandwidth limit)
        assert!(!rendered.contains("meta mark set 10"));
    }

    #[test]
    fn test_bandwidth_requires_qos_block() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                bandwidth {
                    upload_mbps = 5
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        match RouterTemplate::from_hcl(&config) {
            Err(errors) => assert!(errors.iter().any(|e| e.contains("bandwidth requires a qos block"))),
            Ok(_) => panic!("expected error for bandwidth without qos block"),
        }
    }

    #[test]
    fn test_bandwidth_qos_template_htb() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            qos {
                upload_mbps = 20
                download_mbps = 300
            }
            vlan "iot" {
                id = 20
                ipv4 { subnet = "10.20.0.1/24" }
                bandwidth {
                    upload_mbps = 5
                }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let qos_hcl = config.qos.as_ref().unwrap();
        let qos_config = QosConfig::from_hcl(qos_hcl).unwrap();

        let interface_wan = Interface::new("wan").unwrap();
        let vlan_upload_limits = vec![qos::QosVlanBandwidth {
            vlan_id: 20,
            kbit: 5000,
        }];
        let default_upload_kbit = qos_config.upload_kbit - 5000;

        let tmpl = QosTemplate {
            interface_wan,
            upload_kbit: qos_config.upload_kbit,
            download_kbit: qos_config.download_kbit,
            vlan_upload_limits,
            vlan_download_limits: vec![],
            default_upload_kbit,
            default_download_kbit: qos_config.download_kbit,
        };
        let rendered = tmpl.render().unwrap();

        // Should use HTB for upload, not flat CAKE
        assert!(rendered.contains("htb default ffff"));
        assert!(rendered.contains("classid 1:20"));
        assert!(rendered.contains("rate 5000kbit ceil 5000kbit"));
        assert!(rendered.contains("handle 20 fw classid 1:20"));
        // Default class should have remaining bandwidth
        assert!(rendered.contains("classid 1:ffff"));
        assert!(rendered.contains(&format!("rate {}kbit", default_upload_kbit)));
        // Download should be flat CAKE (no download limits)
        assert!(rendered.contains("cake bandwidth 270000kbit diffserv4 nat wash ingress"));
    }

    #[test]
    fn test_bandwidth_qos_template_flat_cake() {
        let hcl = r#"
            interfaces {
                trunk { name = "trunk" }
                wan   { name = "wan" }
            }
            wan { enable_ipv4 = true }
            qos {
                upload_mbps = 20
                download_mbps = 300
            }
            vlan "trusted" {
                id = 10
                ipv4 { subnet = "10.10.0.1/24" }
            }
        "#;
        let config = parse_hcl(hcl).unwrap();
        let qos_hcl = config.qos.as_ref().unwrap();
        let qos_config = QosConfig::from_hcl(qos_hcl).unwrap();

        let tmpl = QosTemplate {
            interface_wan: Interface::new("wan").unwrap(),
            upload_kbit: qos_config.upload_kbit,
            download_kbit: qos_config.download_kbit,
            vlan_upload_limits: vec![],
            vlan_download_limits: vec![],
            default_upload_kbit: qos_config.upload_kbit,
            default_download_kbit: qos_config.download_kbit,
        };
        let rendered = tmpl.render().unwrap();

        // Should use flat CAKE, not HTB
        assert!(!rendered.contains("htb"));
        assert!(rendered.contains("cake bandwidth 18000kbit diffserv4 nat wash\n"));
        assert!(rendered.contains("cake bandwidth 270000kbit diffserv4 nat wash ingress"));
    }

    #[test]
    fn test_bandwidth_download_htb() {
        let qos_hcl = crate::hcl_config::QosHclConfig {
            upload_mbps: 20,
            download_mbps: 300,
            shave_percent: 10,
            overrides: None,
        };
        let qos_config = QosConfig::from_hcl(&qos_hcl).unwrap();

        let tmpl = QosTemplate {
            interface_wan: Interface::new("wan").unwrap(),
            upload_kbit: qos_config.upload_kbit,
            download_kbit: qos_config.download_kbit,
            vlan_upload_limits: vec![],
            vlan_download_limits: vec![qos::QosVlanBandwidth {
                vlan_id: 20,
                kbit: 10000,
            }],
            default_upload_kbit: qos_config.upload_kbit,
            default_download_kbit: qos_config.download_kbit - 10000,
        };
        let rendered = tmpl.render().unwrap();

        // Upload should be flat CAKE
        assert!(rendered.contains("cake bandwidth 18000kbit diffserv4 nat wash\n"));
        // Download should use HTB+CAKE on ifb0
        assert!(rendered.contains("tc qdisc add dev ifb0 root handle 1: htb default ffff"));
        assert!(rendered.contains("classid 1:20"));
        assert!(rendered.contains("rate 10000kbit ceil 10000kbit"));
        // Conntrack mark restoration
        assert!(rendered.contains("matchall action ct"));
        assert!(rendered.contains("handle 20 fw classid 1:20"));
    }
}
