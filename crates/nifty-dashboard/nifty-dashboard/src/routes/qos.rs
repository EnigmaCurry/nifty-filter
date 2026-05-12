use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use tokio::process::Command;

use crate::{
    config_watcher::config_file_path,
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_qos))
}

// --- Response types ---

#[derive(Serialize, JsonSchema)]
struct QosResponse {
    configured: bool,
    active: bool,
    config: Option<QosConfigInfo>,
    upload: Option<CakeStats>,
    download: Option<CakeStats>,
    upload_classes: Vec<CakeClassStats>,
    bandwidth_limits: Vec<BandwidthLimit>,
    dscp_rules: Vec<DscpRule>,
    bandwidth_rules: Vec<DscpRule>,
}

#[derive(Serialize, JsonSchema)]
struct QosConfigInfo {
    upload_mbps: u64,
    download_mbps: u64,
    shave_percent: u64,
    effective_upload_kbit: u64,
    effective_download_kbit: u64,
    wan_interface: String,
    vlan_classes: Vec<VlanQosClass>,
    overrides: Vec<QosOverrideEntry>,
}

#[derive(Serialize, JsonSchema)]
struct VlanQosClass {
    vlan_id: u64,
    name: String,
    qos_class: String,
}

#[derive(Serialize, JsonSchema)]
struct QosOverrideEntry {
    class: String,
    cidrs: String,
}

#[derive(Serialize, JsonSchema)]
struct CakeStats {
    device: String,
    bandwidth: String,
    sent_bytes: u64,
    sent_packets: u64,
    dropped: u64,
    overlimits: u64,
    tins: Vec<CakeTin>,
}

#[derive(Serialize, JsonSchema)]
struct CakeTin {
    name: String,
    threshold: String,
    target: String,
    packets: u64,
    bytes: u64,
    drops: u64,
    marks: u64,
    peak_delay: String,
    avg_delay: String,
    backlog: String,
    sp_flows: u64,
    bk_flows: u64,
}

#[derive(Serialize, JsonSchema)]
struct DscpRule {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// Per-VLAN CAKE stats when HTB+CAKE is in use.
#[derive(Serialize, JsonSchema)]
struct CakeClassStats {
    class_id: String,
    label: String,
    cake: CakeStats,
}

/// Per-VLAN bandwidth limit from HCL config.
#[derive(Serialize, JsonSchema)]
struct BandwidthLimit {
    vlan_id: u64,
    name: String,
    upload_mbps: u64,
}

// --- Handler ---

#[api_doc(
    id = "get_qos",
    tag = "qos",
    ok = "Json<ApiResponse<QosResponse>>",
    err = "Json<ErrorBody>"
)]
/// QoS status
///
/// Returns QoS configuration, CAKE qdisc statistics, and DSCP marking rules.
async fn get_qos(_state: State<AppState>) -> ApiJson<QosResponse> {
    let hcl = read_hcl_config().await;
    let wan_iface = hcl.as_ref()
        .and_then(|v| v.get("interfaces"))
        .and_then(|v| v.get("wan"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("wan")
        .to_string();

    let (config, vlan_names, bandwidth_limits) = extract_qos_config(&hcl, &wan_iface);
    let configured = config.is_some();

    let (wan_upload, download, dscp_rules, bandwidth_rules) = tokio::join!(
        read_wan_upload_stats(&wan_iface),
        read_cake_stats_flat("ifb0"),
        read_dscp_rules(),
        read_bandwidth_rules(),
    );

    let (upload, mut upload_classes) = wan_upload;
    let active = upload.is_some() || !upload_classes.is_empty();

    // Enrich upload_classes labels with VLAN names
    for cls in &mut upload_classes {
        if cls.label.starts_with("VLAN ") {
            if let Some(name) = vlan_names.get(&cls.label[5..]) {
                cls.label = format!("{} (VLAN {})", name, &cls.label[5..]);
            }
        }
    }

    json_ok(QosResponse {
        configured,
        active,
        config,
        upload,
        download,
        upload_classes,
        bandwidth_limits,
        dscp_rules,
        bandwidth_rules,
    })
}

// --- Data collectors ---

/// Read and parse the HCL config file.
async fn read_hcl_config() -> Option<Value> {
    let contents = tokio::fs::read_to_string(config_file_path()).await.ok()?;
    hcl::from_str(&contents).ok()
}

/// Extract QoS configuration, VLAN name map, and bandwidth limits from parsed HCL.
fn extract_qos_config(
    hcl: &Option<Value>,
    wan_iface: &str,
) -> (Option<QosConfigInfo>, HashMap<String, String>, Vec<BandwidthLimit>) {
    let hcl = match hcl {
        Some(v) => v,
        None => return (None, HashMap::new(), vec![]),
    };

    // Build VLAN name map: vlan_id_str → name
    let mut vlan_names: HashMap<String, String> = HashMap::new();
    let mut bandwidth_limits = Vec::new();

    if let Some(vlans) = hcl.get("vlan").and_then(|v| v.as_object()) {
        let mut entries: Vec<_> = vlans.iter().collect();
        entries.sort_by_key(|(_, v)| v.get("id").and_then(|id| id.as_u64()).unwrap_or(0));

        for (name, vlan) in entries {
            if let Some(id) = vlan.get("id").and_then(|v| v.as_u64()) {
                vlan_names.insert(id.to_string(), name.clone());

                if let Some(bw) = vlan.get("bandwidth") {
                    if let Some(upload_mbps) = bw.get("upload_mbps").and_then(|v| v.as_u64()) {
                        bandwidth_limits.push(BandwidthLimit {
                            vlan_id: id,
                            name: name.clone(),
                            upload_mbps,
                        });
                    }
                }
            }
        }
    }

    // Parse QoS block
    let qos = match hcl.get("qos") {
        Some(q) => q,
        None => return (None, vlan_names, bandwidth_limits),
    };

    let upload_mbps = qos.get("upload_mbps").and_then(|v| v.as_u64()).unwrap_or(0);
    let download_mbps = qos.get("download_mbps").and_then(|v| v.as_u64()).unwrap_or(0);
    let shave_percent = qos.get("shave_percent").and_then(|v| v.as_u64()).unwrap_or(10);

    if upload_mbps == 0 || download_mbps == 0 {
        return (None, vlan_names, bandwidth_limits);
    }

    let effective_upload_kbit = upload_mbps * 1000 * (100 - shave_percent) / 100;
    let effective_download_kbit = download_mbps * 1000 * (100 - shave_percent) / 100;

    // Per-VLAN QoS classes
    let mut vlan_classes = Vec::new();
    if let Some(vlans) = hcl.get("vlan").and_then(|v| v.as_object()) {
        let mut entries: Vec<_> = vlans.iter().collect();
        entries.sort_by_key(|(_, v)| v.get("id").and_then(|id| id.as_u64()).unwrap_or(0));

        for (name, vlan) in entries {
            if let Some(qos_class) = vlan.get("qos_class").and_then(|v| v.as_str()) {
                let id = vlan.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                vlan_classes.push(VlanQosClass {
                    vlan_id: id,
                    name: name.clone(),
                    qos_class: qos_class.to_string(),
                });
            }
        }
    }

    // QoS overrides
    let mut overrides = Vec::new();
    if let Some(ovr) = qos.get("overrides") {
        for class in &["voice", "video", "besteffort", "bulk"] {
            if let Some(cidrs) = ovr.get(*class).and_then(|v| v.as_array()) {
                let cidr_strs: Vec<String> = cidrs
                    .iter()
                    .filter_map(|c| c.as_str().map(|s| s.to_string()))
                    .collect();
                if !cidr_strs.is_empty() {
                    overrides.push(QosOverrideEntry {
                        class: class.to_string(),
                        cidrs: cidr_strs.join(", "),
                    });
                }
            }
        }
    }

    let config = QosConfigInfo {
        upload_mbps,
        download_mbps,
        shave_percent,
        effective_upload_kbit,
        effective_download_kbit,
        wan_interface: wan_iface.to_string(),
        vlan_classes,
        overrides,
    };

    (Some(config), vlan_names, bandwidth_limits)
}

/// Run `tc -s qdisc show dev <device>` and return stdout.
async fn run_tc_qdisc_show(device: &str) -> Option<String> {
    if !device
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }

    let output = Command::new("tc")
        .args(["-s", "qdisc", "show", "dev", device])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Read WAN upload stats. Returns (flat_cake, htb_classes).
async fn read_wan_upload_stats(device: &str) -> (Option<CakeStats>, Vec<CakeClassStats>) {
    let stdout = match run_tc_qdisc_show(device).await {
        Some(s) => s,
        None => return (None, vec![]),
    };

    if stdout.contains("qdisc htb") {
        let classes = parse_htb_cake_sections(device, &stdout);
        (None, classes)
    } else if stdout.contains("cake") {
        (parse_cake_section(device, &stdout), vec![])
    } else {
        (None, vec![])
    }
}

/// Read a single flat CAKE qdisc (used for download/ifb0).
async fn read_cake_stats_flat(device: &str) -> Option<CakeStats> {
    let stdout = run_tc_qdisc_show(device).await?;
    if !stdout.contains("cake") {
        return None;
    }
    parse_cake_section(device, &stdout)
}

/// Split tc output into per-qdisc sections.
fn split_qdisc_sections(output: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in output.lines() {
        if line.trim_start().starts_with("qdisc ") && !current.is_empty() {
            sections.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        sections.push(current);
    }
    sections
}

/// Extract the minor classid from a `parent major:minor` in a qdisc line.
fn extract_parent_minor(line: &str) -> Option<String> {
    let parent_idx = line.find("parent ")?;
    let rest = &line[parent_idx + 7..];
    let classid = rest.split_whitespace().next()?;
    let minor = classid.split(':').nth(1)?;
    Some(minor.to_string())
}

/// Parse HTB+CAKE output into per-class stats.
fn parse_htb_cake_sections(device: &str, output: &str) -> Vec<CakeClassStats> {
    let sections = split_qdisc_sections(output);
    let mut classes = Vec::new();

    for section in &sections {
        let trimmed = section.trim_start();
        if !trimmed.starts_with("qdisc cake") {
            continue;
        }

        let first_line = trimmed.lines().next().unwrap_or("");
        let parent_minor = match extract_parent_minor(first_line) {
            Some(m) => m,
            None => continue,
        };

        let label = if parent_minor == "ffff" {
            "Default".to_string()
        } else {
            format!("VLAN {}", parent_minor)
        };

        if let Some(cake) = parse_cake_section(device, section) {
            classes.push(CakeClassStats {
                class_id: format!("1:{}", parent_minor),
                label,
                cake,
            });
        }
    }

    classes
}

/// Parse a single CAKE qdisc section into CakeStats.
fn parse_cake_section(device: &str, output: &str) -> Option<CakeStats> {
    let mut bandwidth = String::new();
    let mut sent_bytes: u64 = 0;
    let mut sent_packets: u64 = 0;
    let mut dropped: u64 = 0;
    let mut overlimits: u64 = 0;

    let tin_names = ["Bulk", "Best Effort", "Video", "Voice"];
    let mut tin_data: Vec<CakeTin> = tin_names
        .iter()
        .map(|n| CakeTin {
            name: n.to_string(),
            threshold: String::new(),
            target: String::new(),
            packets: 0,
            bytes: 0,
            drops: 0,
            marks: 0,
            peak_delay: String::new(),
            avg_delay: String::new(),
            backlog: String::new(),
            sp_flows: 0,
            bk_flows: 0,
        })
        .collect();

    for line in output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("qdisc cake") {
            if let Some(bw_start) = trimmed.find("bandwidth ") {
                let rest = &trimmed[bw_start + 10..];
                bandwidth = rest.split_whitespace().next().unwrap_or("").to_string();
            }
        }

        if trimmed.starts_with("Sent ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                sent_bytes = parts[1].parse().unwrap_or(0);
                sent_packets = parts[3].parse().unwrap_or(0);
            }
            if let Some(d_start) = trimmed.find("dropped ") {
                let rest = &trimmed[d_start + 8..];
                dropped = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
            }
            if let Some(o_start) = trimmed.find("overlimits ") {
                let rest = &trimmed[o_start + 11..];
                overlimits = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
            }
        }

        if let Some(label) = extract_tin_label(trimmed) {
            let values = extract_tin_values(trimmed);
            if values.len() == 4 {
                for (i, val) in values.iter().enumerate() {
                    match label {
                        "thresh" => tin_data[i].threshold = val.clone(),
                        "target" => tin_data[i].target = val.clone(),
                        "pkts" => tin_data[i].packets = val.parse().unwrap_or(0),
                        "bytes" => tin_data[i].bytes = val.parse().unwrap_or(0),
                        "drops" => tin_data[i].drops = val.parse().unwrap_or(0),
                        "marks" => tin_data[i].marks = val.parse().unwrap_or(0),
                        "pk_delay" => tin_data[i].peak_delay = val.clone(),
                        "av_delay" => tin_data[i].avg_delay = val.clone(),
                        "backlog" => tin_data[i].backlog = val.clone(),
                        "sp_flows" => tin_data[i].sp_flows = val.parse().unwrap_or(0),
                        "bk_flows" => tin_data[i].bk_flows = val.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
        }
    }

    Some(CakeStats {
        device: device.to_string(),
        bandwidth,
        sent_bytes,
        sent_packets,
        dropped,
        overlimits,
        tins: tin_data,
    })
}

fn extract_tin_label(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let labels = [
        "thresh", "target", "interval", "pk_delay", "av_delay", "sp_delay",
        "backlog", "pkts", "bytes", "way_inds", "way_miss", "way_cols",
        "drops", "marks", "ack_drop", "sp_flows", "bk_flows", "un_flows",
        "max_len", "quantum",
    ];
    for label in labels {
        if trimmed.starts_with(label) {
            let rest = &trimmed[label.len()..];
            if rest.starts_with(' ') || rest.is_empty() {
                return Some(label);
            }
        }
    }
    None
}

fn extract_tin_values(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 5 {
        parts[1..5].iter().map(|s| s.to_string()).collect()
    } else {
        vec![]
    }
}

/// Parse nftables mangle rules containing `dscp set` (priority marking).
async fn read_dscp_rules() -> Vec<DscpRule> {
    let stdout = read_mangle_table().await;
    stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.contains("dscp set"))
        .map(|l| parse_nft_rule(l))
        .collect()
}

/// Parse nftables mangle rules containing `meta mark set` (bandwidth marking).
async fn read_bandwidth_rules() -> Vec<DscpRule> {
    let stdout = read_mangle_table().await;
    stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.contains("meta mark set"))
        .map(|l| parse_nft_rule(l))
        .collect()
}

async fn read_mangle_table() -> String {
    let output = Command::new("nft")
        .args(["list", "table", "inet", "mangle"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

fn parse_nft_rule(l: &str) -> DscpRule {
    let (text, description) = if let Some(comment_pos) = l.rfind(" comment \"") {
        let after = &l[comment_pos + 10..];
        if let Some(end_quote) = after.rfind('"') {
            let comment = &after[..end_quote];
            let rule_text = l[..comment_pos].to_string();
            let desc = comment
                .strip_prefix("nf:")
                .map(|d| if d.is_empty() { None } else { Some(d.to_string()) })
                .unwrap_or(None);
            (rule_text, desc)
        } else {
            (l.to_string(), None)
        }
    } else {
        (l.to_string(), None)
    };
    DscpRule { text, description }
}
