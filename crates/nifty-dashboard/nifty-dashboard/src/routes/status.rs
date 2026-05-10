use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, get_with_docs};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
struct ConfigResponse {
    config: Value,
    reboot_needed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    boot_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_error: Option<String>,
}

const SENSITIVE_PATTERNS: &[&str] = &["PASS", "SECRET", "TOKEN", "KEY"];

/// Recursively redact sensitive values in a JSON tree.
/// Any object key containing PASS/SECRET/TOKEN/KEY (case-insensitive)
/// has its value replaced with "******".
fn redact_sensitive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let upper = key.to_uppercase();
                if SENSITIVE_PATTERNS.iter().any(|p| upper.contains(p)) {
                    *val = Value::String("******".to_string());
                } else {
                    redact_sensitive(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_sensitive(item);
            }
        }
        _ => {}
    }
}

/// Parse HCL config file contents to generic JSON Value.
pub fn parse_hcl_to_json(contents: &str) -> Result<Value, String> {
    hcl::from_str(contents).map_err(|e| format!("HCL parse error: {e}"))
}

/// Check that configured interfaces exist on the system, or that a links block
/// is present to create them. Returns Some(error_message) if invalid.
fn validate_interfaces(config: &Value) -> Option<String> {
    let has_links = config.get("links").is_some_and(|v| !v.is_null());
    if has_links {
        return None;
    }

    let interfaces = config.get("interfaces")?;
    let mut missing = Vec::new();

    for key in &["wan", "trunk", "mgmt"] {
        if let Some(Value::String(name)) = interfaces.get(key) {
            let sys_path = format!("/sys/class/net/{}", name);
            if !std::path::Path::new(&sys_path).exists() {
                missing.push(name.as_str());
            }
        }
    }

    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "Interface(s) {} not found and no links block to create them. \
             Add a links {{ wan = \"MAC\" trunk = \"MAC\" }} block to your config.",
            missing.join(", ")
        ))
    }
}

#[api_doc(
    id = "get_config",
    tag = "status",
    ok = "Json<ApiResponse<ConfigResponse>>",
    err = "Json<ErrorBody>"
)]
/// Configuration
///
/// Returns the nifty-filter HCL configuration as JSON with sensitive values redacted.
async fn get_config(state: State<AppState>) -> ApiJson<ConfigResponse> {
    let path = crate::config_watcher::config_file_path();
    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return json_ok(ConfigResponse {
                config: Value::Null,
                reboot_needed: false,
                boot_config: None,
                config_error: Some(format!("Cannot read config file: {e}")),
            });
        }
    };

    let current_sha = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(contents.as_bytes()))
    };
    let reboot_needed =
        !state.config_boot_sha.is_empty() && current_sha != state.config_boot_sha;

    let (config, config_error) = match parse_hcl_to_json(&contents) {
        Ok(v) => {
            // Validate that configured interfaces exist or a links block is present
            let validation_error = validate_interfaces(&v);
            (v, validation_error)
        }
        Err(e) => (Value::Null, Some(e)),
    };
    let mut config = config;
    redact_sensitive(&mut config);

    let boot_config = if reboot_needed {
        state.config_boot_values.clone().map(|mut v| {
            redact_sensitive(&mut v);
            v
        })
    } else {
        None
    };

    json_ok(ConfigResponse {
        config,
        reboot_needed,
        boot_config,
        config_error,
    })
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
