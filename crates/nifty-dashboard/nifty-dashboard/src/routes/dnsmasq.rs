use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_dnsmasq))
}

// --- Response types ---

#[derive(Serialize, JsonSchema)]
struct DnsmasqResponse {
    /// Whether the dnsmasq config file exists
    config_found: bool,
    /// Upstream DNS servers
    upstream_dns: Vec<String>,
    /// Per-interface DHCP configuration
    interfaces: Vec<DnsmasqInterface>,
    /// Static DHCP host reservations
    static_hosts: Vec<DhcpHost>,
    /// Active DHCP leases
    leases: Vec<DhcpLease>,
}

#[derive(Serialize, JsonSchema)]
struct DnsmasqInterface {
    name: String,
    listen_address: Option<String>,
    pool_start: Option<String>,
    pool_end: Option<String>,
    lease_time: Option<String>,
    dhcp_router: Option<String>,
    dhcp_dns: Option<String>,
    pool_start_v6: Option<String>,
    pool_end_v6: Option<String>,
    dhcpv6_dns: Option<String>,
    ra_enabled: bool,
}

#[derive(Serialize, JsonSchema)]
struct DhcpHost {
    mac: String,
    ip: String,
    hostname: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct DhcpLease {
    expires: String,
    mac: String,
    ip: String,
    hostname: String,
    client_id: String,
}

// --- Handler ---

#[api_doc(
    id = "get_dnsmasq",
    tag = "dnsmasq",
    ok = "Json<ApiResponse<DnsmasqResponse>>",
    err = "Json<ErrorBody>"
)]
/// Dnsmasq status
///
/// Returns dnsmasq configuration (parsed from /run/dnsmasq.conf) and active DHCP leases.
async fn get_dnsmasq(_state: State<AppState>) -> ApiJson<DnsmasqResponse> {
    let (config, leases) = tokio::join!(read_dnsmasq_config(), read_leases());

    let (config_found, upstream_dns, interfaces, static_hosts) = config.unwrap_or_default();

    json_ok(DnsmasqResponse {
        config_found,
        upstream_dns,
        interfaces,
        static_hosts,
        leases,
    })
}

// --- Data collectors ---

async fn read_dnsmasq_config() -> Option<(bool, Vec<String>, Vec<DnsmasqInterface>, Vec<DhcpHost>)> {
    let contents = match tokio::fs::read_to_string("/run/dnsmasq.conf").await {
        Ok(c) => c,
        Err(_) => return Some((false, vec![], vec![], vec![])),
    };

    let mut upstream_dns = Vec::new();
    let mut interfaces: Vec<DnsmasqInterface> = Vec::new();
    let mut static_hosts = Vec::new();
    let mut current_iface: Option<&mut DnsmasqInterface> = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(val) = line.strip_prefix("server=") {
            upstream_dns.push(val.to_string());
        } else if let Some(val) = line.strip_prefix("interface=") {
            interfaces.push(DnsmasqInterface {
                name: val.to_string(),
                listen_address: None,
                pool_start: None,
                pool_end: None,
                lease_time: None,
                dhcp_router: None,
                dhcp_dns: None,
                pool_start_v6: None,
                pool_end_v6: None,
                dhcpv6_dns: None,
                ra_enabled: false,
            });
            current_iface = interfaces.last_mut();
        } else if let Some(val) = line.strip_prefix("listen-address=") {
            // Skip localhost listeners
            if val != "127.0.0.1" && val != "::1" {
                if let Some(ref mut iface) = current_iface {
                    iface.listen_address = Some(val.to_string());
                }
            }
        } else if let Some(val) = line.strip_prefix("dhcp-range=") {
            if let Some(ref mut iface) = current_iface {
                // Parse: interface:<name>,<start>,<end>,<prefix_or_lease>[,<lease>]
                let parts: Vec<&str> = val.split(',').collect();
                // Detect IPv6 by presence of "::" in the range
                if val.contains("::") {
                    // IPv6: interface:<name>,<start>,<end>,<prefix_len>,<lease>
                    if parts.len() >= 3 {
                        iface.pool_start_v6 = Some(parts[1].to_string());
                        iface.pool_end_v6 = Some(parts[2].to_string());
                    }
                } else {
                    // IPv4: interface:<name>,<start>,<end>,<lease>
                    if parts.len() >= 3 {
                        iface.pool_start = Some(parts[1].to_string());
                        iface.pool_end = Some(parts[2].to_string());
                        iface.lease_time = parts.last().and_then(|v| {
                            // Last part is lease time if it contains h/m/s or is a number
                            if v.ends_with('h') || v.ends_with('m') || v.ends_with('s') || v.parse::<u64>().is_ok() {
                                Some(v.to_string())
                            } else {
                                None
                            }
                        });
                    }
                }
            }
        } else if let Some(val) = line.strip_prefix("dhcp-option=") {
            if let Some(ref mut iface) = current_iface {
                if val.contains("option:router,") {
                    if let Some(router) = val.rsplit(',').next() {
                        iface.dhcp_router = Some(router.to_string());
                    }
                } else if val.contains("option:dns-server,") {
                    if let Some(dns) = val.rsplit(',').next() {
                        iface.dhcp_dns = Some(dns.to_string());
                    }
                } else if val.contains("option6:dns-server,") {
                    if let Some(dns) = val.rsplit(',').next() {
                        iface.dhcpv6_dns = Some(dns.trim_matches('[').trim_matches(']').to_string());
                    }
                }
            }
        } else if line == "enable-ra" {
            if let Some(ref mut iface) = current_iface {
                iface.ra_enabled = true;
            }
        } else if let Some(val) = line.strip_prefix("dhcp-host=") {
            let parts: Vec<&str> = val.split(',').collect();
            if parts.len() >= 2 {
                static_hosts.push(DhcpHost {
                    mac: parts[0].to_string(),
                    ip: parts[1].to_string(),
                    hostname: parts.get(2).map(|s| s.to_string()),
                });
            }
        }
    }

    Some((true, upstream_dns, interfaces, static_hosts))
}

async fn read_leases() -> Vec<DhcpLease> {
    let contents = match tokio::fs::read_to_string("/var/lib/dnsmasq/dnsmasq.leases").await {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    contents
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                Some(DhcpLease {
                    expires: parts[0].to_string(),
                    mac: parts[1].to_string(),
                    ip: parts[2].to_string(),
                    hostname: parts[3].to_string(),
                    client_id: parts[4].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}
