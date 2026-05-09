use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, get_with_docs};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tokio::process::Command;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_error, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route("/", get_with_docs!(get_status))
        .api_route("/about", get_with_docs!(get_about))
        .api_route("/config", get_with_docs!(get_config))
        .api_route("/nft-rules", get_with_docs!(get_nft_rules))
}

fn state_file_path() -> PathBuf {
    std::env::var("SODOLA_STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/sodola-switch-state.json"))
}

// --- Response types ---

#[derive(Serialize, JsonSchema)]
struct StatusResponse {
    uptime: Option<UptimeInfo>,
    interfaces: Vec<NetworkInterface>,
    nft_chains: Vec<NftChain>,
    switch: Option<SwitchState>,
}

#[derive(Serialize, JsonSchema)]
struct UptimeInfo {
    uptime_seconds: f64,
}

#[derive(Serialize, JsonSchema)]
struct NetworkInterface {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mtu: Option<u64>,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mac: Option<String>,
    addresses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_kind: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct NftChain {
    family: String,
    table: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct SwitchState {
    timestamp: u64,
    info: SwitchInfo,
    stats: Vec<PortStats>,
    vlans: Vec<VlanEntry>,
    pvid: Vec<PortVlanSetting>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct SwitchInfo {
    device_type: String,
    mac_address: String,
    ip_address: String,
    netmask: String,
    gateway: String,
    firmware_version: String,
    firmware_date: String,
    hardware_version: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct PortStats {
    port: u8,
    enabled: bool,
    link_up: bool,
    tx_good: u64,
    tx_bad: u64,
    rx_good: u64,
    rx_bad: u64,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct VlanEntry {
    vid: u16,
    name: String,
    member_ports: String,
    tagged_ports: String,
    untagged_ports: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct PortVlanSetting {
    port: u8,
    pvid: u16,
    accepted_frame_type: String,
}

// --- Handler ---

#[api_doc(
    id = "get_status",
    tag = "status",
    ok = "Json<ApiResponse<StatusResponse>>",
    err = "Json<ErrorBody>"
)]
/// System status
///
/// Returns system uptime, network interfaces, nftables chains, and managed switch state.
async fn get_status(_state: State<AppState>) -> ApiJson<StatusResponse> {
    let (uptime, interfaces, nft_chains, switch) = tokio::join!(
        read_uptime(),
        read_interfaces(),
        read_nft_chains(),
        read_switch_state(),
    );

    json_ok(StatusResponse {
        uptime,
        interfaces,
        nft_chains,
        switch,
    })
}

#[derive(Serialize, JsonSchema)]
struct AboutResponse {
    version: String,
    repository: String,
    license: String,
}

#[api_doc(
    id = "get_about",
    tag = "status",
    ok = "Json<ApiResponse<AboutResponse>>",
    err = "Json<ErrorBody>"
)]
/// About
///
/// Returns version, repository URL, and license text.
async fn get_about(_state: State<AppState>) -> ApiJson<AboutResponse> {
    json_ok(AboutResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        repository: "https://github.com/EnigmaCurry/nifty-filter".to_string(),
        license: include_str!(concat!(env!("OUT_DIR"), "/LICENSE.md")).to_string(),
    })
}

#[derive(Serialize, JsonSchema)]
struct ConfigEntry {
    key: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    is_commented_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    boot_value: Option<String>,
    /// True when this entry is not in the config file and shows the built-in default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_default: bool,
}

#[derive(Serialize, JsonSchema)]
struct ConfigSection {
    name: String,
    entries: Vec<ConfigEntry>,
}

#[derive(Serialize, JsonSchema)]
struct ConfigResponse {
    sections: Vec<ConfigSection>,
    reboot_needed: bool,
}

const SENSITIVE_PATTERNS: &[&str] = &["PASS", "SECRET", "TOKEN", "KEY"];

fn is_sensitive(key: &str) -> bool {
    let upper = key.to_uppercase();
    SENSITIVE_PATTERNS.iter().any(|p| upper.contains(p))
}

fn config_file_path() -> PathBuf {
    std::env::var("NIFTY_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/nifty-filter/nifty-filter.env"))
}

/// Parse config text into a flat map of key → (value, is_commented_out).
/// Used to snapshot boot config and compare against current.
pub fn parse_config_values(contents: &str) -> HashMap<String, (String, bool)> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim();
            if let Some(eq_pos) = rest.find('=') {
                let key = rest[..eq_pos].trim();
                if !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && key.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
                {
                    let value = rest[eq_pos + 1..].trim().trim_matches('"').to_string();
                    let value = if is_sensitive(key) {
                        "******".to_string()
                    } else {
                        value
                    };
                    map.insert(key.to_string(), (value, true));
                }
            }
        } else if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim().trim_matches('"').to_string();
            let value = if is_sensitive(key) {
                "******".to_string()
            } else {
                value
            };
            map.insert(key.to_string(), (value, false));
        }
    }
    map
}

#[api_doc(
    id = "get_config",
    tag = "status",
    ok = "Json<ApiResponse<ConfigResponse>>",
    err = "Json<ErrorBody>"
)]
/// Configuration
///
/// Returns the nifty-filter configuration with sensitive values redacted.
async fn get_config(state: State<AppState>) -> ApiJson<ConfigResponse> {
    let path = config_file_path();
    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(_) => {
            return json_ok(ConfigResponse {
                sections: vec![],
                reboot_needed: false,
            });
        }
    };

    let current_sha = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(contents.as_bytes()))
    };
    let reboot_needed =
        !state.config_boot_sha.is_empty() && current_sha != state.config_boot_sha;
    let boot_vals = &state.config_boot_values;

    let mut sections: Vec<ConfigSection> = Vec::new();
    let mut current_section = ConfigSection {
        name: "General".to_string(),
        entries: Vec::new(),
    };
    let mut pending_comment: Option<String> = None;
    let mut pending_comment_first_line: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Blank line: flush pending comment and possibly start new section
            if !current_section.entries.is_empty() || pending_comment_first_line.is_some() {
                if let Some(first_line) = pending_comment_first_line.take() {
                    pending_comment = None;
                    // Standalone comment block before a blank line = section header candidate
                    if !current_section.entries.is_empty() {
                        sections.push(current_section);
                        current_section = ConfigSection {
                            name: first_line,
                            entries: Vec::new(),
                        };
                    } else {
                        current_section.name = first_line;
                    }
                }
            }
            continue;
        }

        // Commented-out variable: #KEY=value
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim();
            if let Some(eq_pos) = rest.find('=') {
                let key = rest[..eq_pos].trim();
                // Only treat as commented-out var if key looks like an env var name
                if !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && key.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
                {
                    let value = rest[eq_pos + 1..].trim().trim_matches('"').to_string();
                    let display_value = if is_sensitive(key) {
                        "******".to_string()
                    } else {
                        value
                    };
                    // Show boot_value if the entry was different at boot
                    let boot_value = if reboot_needed {
                        match boot_vals.get(key) {
                            Some((bv, bc)) => {
                                if *bc != true || *bv != display_value {
                                    Some(if *bc {
                                        format!("#{bv}")
                                    } else {
                                        bv.clone()
                                    })
                                } else {
                                    None
                                }
                            }
                            // Key not in boot snapshot = new since boot
                            None => Some("(new)".to_string()),
                        }
                    } else {
                        None
                    };
                    current_section.entries.push(ConfigEntry {
                        key: key.to_string(),
                        value: display_value,
                        comment: pending_comment.take(),
                        is_commented_out: true,
                        boot_value,
                        is_default: false,
                    });
                    pending_comment_first_line = None;
                    continue;
                }
            }
            // Regular comment line — accumulate as pending comment
            let comment_text = rest.to_string();
            if !comment_text.is_empty() {
                if pending_comment_first_line.is_none() {
                    pending_comment_first_line = Some(comment_text.clone());
                }
                match &mut pending_comment {
                    Some(existing) => {
                        existing.push(' ');
                        existing.push_str(&comment_text);
                    }
                    None => {
                        pending_comment = Some(comment_text);
                    }
                }
            }
            continue;
        }

        // Active variable: KEY=value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim().trim_matches('"').to_string();
            let display_value = if is_sensitive(key) {
                "******".to_string()
            } else {
                value
            };
            // Show boot_value if the entry was different at boot
            let boot_value = if reboot_needed {
                match boot_vals.get(key) {
                    Some((bv, bc)) => {
                        if *bc != false || *bv != display_value {
                            Some(if *bc {
                                format!("#{bv}")
                            } else {
                                bv.clone()
                            })
                        } else {
                            None
                        }
                    }
                    // Key not in boot snapshot = new since boot
                    None => Some("(new)".to_string()),
                }
            } else {
                None
            };
            current_section.entries.push(ConfigEntry {
                key: key.to_string(),
                value: display_value,
                comment: pending_comment.take(),
                is_commented_out: false,
                boot_value,
                is_default: false,
            });
            pending_comment_first_line = None;
        }
    }

    if !current_section.entries.is_empty() {
        sections.push(current_section);
    }

    // Show vars that existed at boot but were removed from the current config
    if reboot_needed {
        let current_keys: HashSet<&str> = sections
            .iter()
            .flat_map(|s| s.entries.iter())
            .map(|e| e.key.as_str())
            .collect();
        let mut removed_entries: Vec<ConfigEntry> = boot_vals
            .iter()
            .filter(|(k, _)| !current_keys.contains(k.as_str()))
            .map(|(k, (v, commented))| ConfigEntry {
                key: k.clone(),
                value: "(removed)".to_string(),
                comment: None,
                is_commented_out: *commented,
                boot_value: Some(if *commented {
                    format!("#{v}")
                } else {
                    v.clone()
                }),
                is_default: false,
            })
            .collect();
        if !removed_entries.is_empty() {
            removed_entries.sort_by(|a, b| a.key.cmp(&b.key));
            sections.push(ConfigSection {
                name: "Removed since boot".to_string(),
                entries: removed_entries,
            });
        }
    }

    // Inject defaults for known vars not present in the config file
    inject_defaults(&mut sections);

    json_ok(ConfigResponse {
        sections,
        reboot_needed,
    })
}

/// Built-in defaults for known nifty-filter environment variables.
/// Returns (key, default_value) pairs for static vars, plus generates
/// per-VLAN defaults based on which VLANs are configured.
fn known_defaults(sections: &[ConfigSection]) -> Vec<(&'static str, String)> {
    // Collect all current keys for lookups
    let vals: HashMap<&str, &str> = sections
        .iter()
        .flat_map(|s| s.entries.iter())
        .filter(|e| !e.is_commented_out)
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();

    let mut defaults: Vec<(&str, String)> = Vec::new();

    // Static defaults
    let static_defaults: &[(&str, &str)] = &[
        ("WAN_ENABLE_IPV4", "true"),
        ("WAN_ENABLE_IPV6", "false"),
        ("VLAN_AWARE_SWITCH", "false"),
        ("IPERF_PORT", "5201"),
        ("WAN_QOS_SHAVE_PERCENT", "10"),
        ("WAN_ICMP_ACCEPT", ""),
        (
            "WAN_ICMPV6_ACCEPT",
            "nd-neighbor-solicit,nd-neighbor-advert,nd-router-solicit,nd-router-advert,destination-unreachable,packet-too-big,time-exceeded",
        ),
        ("WAN_TCP_ACCEPT", ""),
        ("WAN_UDP_ACCEPT", ""),
        ("WAN_TCP_FORWARD", ""),
        ("WAN_UDP_FORWARD", ""),
        (
            "WAN_BOGONS_IPV4",
            "0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24, 224.0.0.0/4, 240.0.0.0/4",
        ),
        ("WAN_BOGONS_IPV6", "::/128, ::1/128, fc00::/7, ff00::/8"),
    ];

    for (key, val) in static_defaults {
        if !vals.contains_key(key) {
            defaults.push((key, val.to_string()));
        }
    }

    // Per-VLAN defaults — only for VLANs already in the config
    let vlans_str = vals.get("VLANS").copied().unwrap_or("");
    if !vlans_str.is_empty() {
        for id_str in vlans_str.split(',') {
            let id_str = id_str.trim();
            let vlan_id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let is_vlan_1 = vlan_id == 1;

            // We use a macro-like approach with a static list per VLAN
            let vlan_defaults: Vec<(String, String)> = vec![
                (format!("VLAN_{}_NAME", vlan_id), String::new()),
                (format!("VLAN_{}_SUBNET_IPV6", vlan_id), String::new()),
                (
                    format!("VLAN_{}_EGRESS_ALLOWED_IPV4", vlan_id),
                    if is_vlan_1 {
                        "0.0.0.0/0".to_string()
                    } else {
                        String::new()
                    },
                ),
                (
                    format!("VLAN_{}_EGRESS_ALLOWED_IPV6", vlan_id),
                    if is_vlan_1 {
                        "::/0".to_string()
                    } else {
                        String::new()
                    },
                ),
                (
                    format!("VLAN_{}_ICMP_ACCEPT", vlan_id),
                    if is_vlan_1 {
                        "echo-request,echo-reply,destination-unreachable,time-exceeded".to_string()
                    } else {
                        "destination-unreachable".to_string()
                    },
                ),
                (
                    format!("VLAN_{}_ICMPV6_ACCEPT", vlan_id),
                    if is_vlan_1 {
                        "nd-neighbor-solicit,nd-neighbor-advert,nd-router-solicit,nd-router-advert,echo-request,echo-reply".to_string()
                    } else {
                        "nd-neighbor-solicit,nd-neighbor-advert,destination-unreachable".to_string()
                    },
                ),
                (
                    format!("VLAN_{}_TCP_ACCEPT", vlan_id),
                    if is_vlan_1 { "22".to_string() } else { String::new() },
                ),
                (
                    format!("VLAN_{}_UDP_ACCEPT", vlan_id),
                    "67,68".to_string(),
                ),
                (format!("VLAN_{}_TCP_FORWARD", vlan_id), String::new()),
                (format!("VLAN_{}_UDP_FORWARD", vlan_id), String::new()),
                (
                    format!("VLAN_{}_DHCP_ENABLED", vlan_id),
                    "true".to_string(),
                ),
                (format!("VLAN_{}_DHCP_POOL_START", vlan_id), String::new()),
                (format!("VLAN_{}_DHCP_POOL_END", vlan_id), String::new()),
                (format!("VLAN_{}_DHCP_ROUTER", vlan_id), String::new()),
                (format!("VLAN_{}_DHCP_DNS", vlan_id), String::new()),
                (
                    format!("VLAN_{}_DHCPV6_ENABLED", vlan_id),
                    "false".to_string(),
                ),
                (
                    format!("VLAN_{}_DHCPV6_POOL_START", vlan_id),
                    String::new(),
                ),
                (format!("VLAN_{}_DHCPV6_POOL_END", vlan_id), String::new()),
                (
                    format!("VLAN_{}_IPERF_ENABLED", vlan_id),
                    "false".to_string(),
                ),
                (format!("VLAN_{}_QOS_CLASS", vlan_id), String::new()),
                (format!("VLAN_{}_ALLOW_INBOUND_TCP", vlan_id), String::new()),
                (format!("VLAN_{}_ALLOW_INBOUND_UDP", vlan_id), String::new()),
            ];

            for (key, val) in vlan_defaults {
                if !vals.contains_key(key.as_str()) {
                    // Leak the string so we can return &'static str — these are few and bounded
                    defaults.push((Box::leak(key.into_boxed_str()), val));
                }
            }

            // Inter-VLAN allow rules: VLAN_N_ALLOW_FROM_M_TCP/UDP for each other VLAN
            for other_str in vlans_str.split(',') {
                let other_str = other_str.trim();
                let other_id: u16 = match other_str.parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if other_id == vlan_id {
                    continue;
                }
                for suffix in ["TCP", "UDP"] {
                    let key = format!("VLAN_{}_ALLOW_FROM_{}_{}", vlan_id, other_id, suffix);
                    if !vals.contains_key(key.as_str()) {
                        defaults.push((Box::leak(key.into_boxed_str()), String::new()));
                    }
                }
            }
        }
    }

    defaults
}

/// Inject default-value entries into the appropriate sections for vars not in the config file.
fn inject_defaults(sections: &mut Vec<ConfigSection>) {
    let defaults = known_defaults(sections);
    if defaults.is_empty() {
        return;
    }

    // Build a set of existing keys (owned to avoid borrow conflicts)
    let existing: HashSet<String> = sections
        .iter()
        .flat_map(|s| s.entries.iter())
        .map(|e| e.key.clone())
        .collect();

    // Collect entries to insert: (prefix, entry)
    let to_insert: Vec<(String, ConfigEntry)> = defaults
        .into_iter()
        .filter(|(key, _)| !existing.contains(*key))
        .map(|(key, value)| {
            let parts: Vec<&str> = key.split('_').collect();
            let prefix = if parts.first() == Some(&"VLAN") && parts.len() >= 2 {
                format!("{}_{}", parts[0], parts[1])
            } else {
                parts[0].to_string()
            };
            (
                prefix,
                ConfigEntry {
                    key: key.to_string(),
                    value,
                    comment: None,
                    is_commented_out: false,
                    boot_value: None,
                    is_default: true,
                },
            )
        })
        .collect();

    for (prefix, entry) in to_insert {
        if let Some(section) = sections.iter_mut().find(|s| s.name == prefix) {
            section.entries.push(entry);
        } else if let Some(general) = sections.iter_mut().find(|s| s.name == "General") {
            general.entries.push(entry);
        } else {
            sections.insert(
                0,
                ConfigSection {
                    name: "General".to_string(),
                    entries: vec![entry],
                },
            );
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct NftRulesQuery {
    family: String,
    table: String,
    chain: String,
}

#[derive(Serialize, JsonSchema)]
struct NftRule {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    in_source: bool,
}

#[derive(Serialize, JsonSchema)]
struct NftRulesResponse {
    family: String,
    table: String,
    chain: String,
    rules: Vec<NftRule>,
}

#[api_doc(
    id = "get_nft_rules",
    tag = "status",
    ok = "Json<ApiResponse<NftRulesResponse>>",
    err = "Json<ErrorBody>"
)]
/// nftables chain rules
///
/// Returns the rules for a specific nftables chain with source descriptions.
async fn get_nft_rules(
    _state: State<AppState>,
    NoApi(Query(q)): NoApi<Query<NftRulesQuery>>,
) -> ApiJson<NftRulesResponse> {
    // Validate inputs contain only safe characters (alphanumeric, underscore, hyphen)
    let safe = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if !safe(&q.family) || !safe(&q.table) || !safe(&q.chain) {
        return json_error(StatusCode::BAD_REQUEST, "invalid parameter characters");
    }

    let output = Command::new("nft")
        .args(["list", "chain", &q.family, &q.table, &q.chain])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            return json_error(
                StatusCode::NOT_FOUND,
                format!("chain not found: {}", stderr.trim()),
            );
        }
        Err(e) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("nft error: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    let rules: Vec<NftRule> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("table ")
                && !l.starts_with("chain ")
                && !l.starts_with("type ")
                && *l != "}"
        })
        .map(|l| parse_nft_rule(l))
        .collect();

    json_ok(NftRulesResponse {
        family: q.family,
        table: q.table,
        chain: q.chain,
        rules,
    })
}

/// Parse a rule line, extracting `comment "nf:..."` if present.
/// Returns the rule text without the comment annotation, the description, and in_source flag.
fn parse_nft_rule(rule: &str) -> NftRule {
    // nft outputs: ... comment "nf:Some description"
    // or: ... comment "nf:"
    // Find the last `comment "` in the rule
    if let Some(comment_pos) = rule.rfind(" comment \"") {
        let after = &rule[comment_pos + 10..]; // skip ` comment "`
        if let Some(end_quote) = after.rfind('"') {
            let comment_value = &after[..end_quote];
            let rule_text = rule[..comment_pos].to_string();

            if let Some(desc) = comment_value.strip_prefix("nf:") {
                let description = if desc.is_empty() {
                    None
                } else {
                    Some(desc.to_string())
                };
                return NftRule {
                    text: rule_text,
                    description,
                    in_source: true,
                };
            }
        }
    }

    // No nf: comment found — this rule was added outside the template
    NftRule {
        text: rule.to_string(),
        description: None,
        in_source: false,
    }
}

// --- Data collectors ---

async fn read_uptime() -> Option<UptimeInfo> {
    let contents = tokio::fs::read_to_string("/proc/uptime").await.ok()?;
    let mut parts = contents.split_whitespace();
    let uptime_seconds: f64 = parts.next()?.parse().ok()?;
    Some(UptimeInfo { uptime_seconds })
}

async fn read_interfaces() -> Vec<NetworkInterface> {
    let output = Command::new("ip")
        .args(["-j", "addr", "show"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let parsed: Vec<serde_json::Value> = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    const HIDDEN_IFACES: &[&str] = &["ip6tnl0", "sit0", "tunl0", "ip6gre0", "gre0", "erspan0", "ifb0"];

    parsed
        .iter()
        .filter(|iface| {
            let name = iface["ifname"].as_str().unwrap_or("");
            !HIDDEN_IFACES.contains(&name)
        })
        .map(|iface| {
            let name = iface["ifname"].as_str().unwrap_or("").to_string();
            let mtu = iface["mtu"].as_u64();
            let state = iface["operstate"].as_str().unwrap_or("UNKNOWN").to_string();
            let mac = iface["address"].as_str().map(|s| s.to_string());
            let link_kind = iface["linkinfo"]["info_kind"]
                .as_str()
                .map(|s| s.to_string());

            let addresses = iface["addr_info"]
                .as_array()
                .map(|addrs| {
                    addrs
                        .iter()
                        .filter_map(|a| {
                            let local = a["local"].as_str()?;
                            let prefixlen = a["prefixlen"].as_u64().unwrap_or(0);
                            Some(format!("{}/{}", local, prefixlen))
                        })
                        .collect()
                })
                .unwrap_or_default();

            NetworkInterface {
                name,
                mtu,
                state,
                mac,
                addresses,
                link_kind,
            }
        })
        .collect()
}

async fn read_nft_chains() -> Vec<NftChain> {
    let output = Command::new("nft")
        .args(["-j", "list", "chains"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let nftables = match parsed["nftables"].as_array() {
        Some(arr) => arr,
        None => return vec![],
    };

    nftables
        .iter()
        .filter_map(|item| {
            let chain = item.get("chain")?;
            Some(NftChain {
                family: chain["family"].as_str()?.to_string(),
                table: chain["table"].as_str()?.to_string(),
                name: chain["name"].as_str()?.to_string(),
                chain_type: chain["type"].as_str().map(|s| s.to_string()),
                hook: chain["hook"].as_str().map(|s| s.to_string()),
                priority: chain["prio"].as_i64(),
                policy: chain["policy"].as_str().map(|s| s.to_string()),
            })
        })
        .collect()
}

async fn read_switch_state() -> Option<SwitchState> {
    const MAX_AGE_SECS: u64 = 300;
    let path = state_file_path();
    let contents = tokio::fs::read_to_string(&path).await.ok()?;
    let state: SwitchState = serde_json::from_str(&contents).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(state.timestamp) > MAX_AGE_SECS {
        return None;
    }
    Some(state)
}
