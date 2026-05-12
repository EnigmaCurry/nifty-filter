use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::PathBuf;
use tokio::process::Command;

use crate::{
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
    dscp_rules: Vec<DscpRule>,
    bandwidth_rules: Vec<DscpRule>,
}

#[derive(Serialize, JsonSchema)]
struct QosConfigInfo {
    upload_mbps: String,
    download_mbps: String,
    shave_percent: String,
    effective_upload_kbit: u64,
    effective_download_kbit: u64,
    wan_interface: String,
    vlan_classes: Vec<VlanQosClass>,
    overrides: Vec<QosOverrideEntry>,
}

#[derive(Serialize, JsonSchema)]
struct VlanQosClass {
    vlan_id: String,
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
    let config = read_qos_config().await;
    let wan_iface = config
        .as_ref()
        .map(|c| c.wan_interface.clone())
        .unwrap_or_else(|| "wan".to_string());

    let configured = config.is_some();

    let (wan_upload, download, dscp_rules, bandwidth_rules) = tokio::join!(
        read_wan_upload_stats(&wan_iface),
        read_cake_stats_flat("ifb0"),
        read_dscp_rules(),
        read_bandwidth_rules(),
    );

    let (upload, upload_classes) = wan_upload;
    let active = upload.is_some() || !upload_classes.is_empty();

    json_ok(QosResponse {
        configured,
        active,
        config,
        upload,
        download,
        upload_classes,
        dscp_rules,
        bandwidth_rules,
    })
}

// --- Data collectors ---

fn config_file_path() -> PathBuf {
    std::env::var("NIFTY_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/nifty-filter/nifty-filter.env"))
}

async fn read_qos_config() -> Option<QosConfigInfo> {
    let contents = tokio::fs::read_to_string(config_file_path()).await.ok()?;

    let get_val = |key: &str| -> Option<String> {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                if trimmed[..eq_pos].trim() == key {
                    let val = trimmed[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                    return Some(val.to_string());
                }
            }
        }
        None
    };

    let upload_mbps = get_val("WAN_QOS_UPLOAD_MBPS")?;
    let download_mbps = get_val("WAN_QOS_DOWNLOAD_MBPS")?;
    let wan_interface = get_val("WAN_INTERFACE").unwrap_or_else(|| "wan".to_string());
    let shave_percent = get_val("WAN_QOS_SHAVE_PERCENT").unwrap_or_else(|| "10".to_string());

    let upload: u64 = upload_mbps.parse().ok()?;
    let download: u64 = download_mbps.parse().ok()?;
    let shave: u64 = shave_percent.parse().unwrap_or(10);

    let effective_upload_kbit = upload * 1000 * (100 - shave) / 100;
    let effective_download_kbit = download * 1000 * (100 - shave) / 100;

    // Parse per-VLAN QoS classes
    let vlans_str = get_val("VLANS").unwrap_or_default();
    let vlan_ids: Vec<&str> = vlans_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    let vlan_classes: Vec<VlanQosClass> = vlan_ids
        .iter()
        .filter_map(|id| {
            let qos_class = get_val(&format!("VLAN_{}_QOS_CLASS", id))?;
            let name = get_val(&format!("VLAN_{}_NAME", id)).unwrap_or_else(|| format!("VLAN {}", id));
            Some(VlanQosClass {
                vlan_id: id.to_string(),
                name,
                qos_class,
            })
        })
        .collect();

    // Parse QoS overrides
    let override_classes = ["VOICE", "VIDEO", "BESTEFFORT", "BULK"];
    let overrides: Vec<QosOverrideEntry> = override_classes
        .iter()
        .filter_map(|class| {
            let cidrs = get_val(&format!("QOS_OVERRIDE_{}", class))?;
            if cidrs.is_empty() {
                return None;
            }
            Some(QosOverrideEntry {
                class: class.to_lowercase(),
                cidrs,
            })
        })
        .collect();

    Some(QosConfigInfo {
        upload_mbps,
        download_mbps,
        shave_percent,
        effective_upload_kbit,
        effective_download_kbit,
        wan_interface,
        vlan_classes,
        overrides,
    })
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
/// When HTB+CAKE is active, flat_cake is None and htb_classes is populated.
/// When flat CAKE is active, flat_cake is populated and htb_classes is empty.
async fn read_wan_upload_stats(device: &str) -> (Option<CakeStats>, Vec<CakeClassStats>) {
    let stdout = match run_tc_qdisc_show(device).await {
        Some(s) => s,
        None => return (None, vec![]),
    };

    if stdout.contains("qdisc htb") {
        // HTB+CAKE mode: parse per-class CAKE qdiscs
        let classes = parse_htb_cake_sections(device, &stdout);
        (None, classes)
    } else if stdout.contains("cake") {
        // Flat CAKE mode
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

    // Parse per-tin table data
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

        // Extract bandwidth from first line
        if trimmed.starts_with("qdisc cake") {
            if let Some(bw_start) = trimmed.find("bandwidth ") {
                let rest = &trimmed[bw_start + 10..];
                bandwidth = rest.split_whitespace().next().unwrap_or("").to_string();
            }
        }

        // Parse "Sent N bytes M pkt (dropped D, overlimits O requeues R)"
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

        // Parse tin table rows — each row has a label and 4 values
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

/// Extract the label from a tc CAKE tin stats line (e.g., "thresh" from "  thresh  1125Kbit  18Mbit ...")
fn extract_tin_label(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let labels = [
        "thresh",
        "target",
        "interval",
        "pk_delay",
        "av_delay",
        "sp_delay",
        "backlog",
        "pkts",
        "bytes",
        "way_inds",
        "way_miss",
        "way_cols",
        "drops",
        "marks",
        "ack_drop",
        "sp_flows",
        "bk_flows",
        "un_flows",
        "max_len",
        "quantum",
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

/// Extract the 4 tin values from a tc CAKE stats line
fn extract_tin_values(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    // Skip the label, then collect remaining whitespace-separated values
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 5 {
        // Label + 4 values
        parts[1..5].iter().map(|s| s.to_string()).collect()
    } else {
        vec![]
    }
}

/// Parse nftables mangle rules containing `dscp set` (priority marking).
async fn read_dscp_rules() -> Vec<DscpRule> {
    let output = Command::new("nft")
        .args(["list", "table", "inet", "mangle"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.contains("dscp set"))
        .map(|l| parse_nft_rule(l))
        .collect()
}

/// Parse nftables mangle rules containing `meta mark set` (bandwidth marking).
async fn read_bandwidth_rules() -> Vec<DscpRule> {
    let output = Command::new("nft")
        .args(["list", "table", "inet", "mangle"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.contains("meta mark set"))
        .map(|l| parse_nft_rule(l))
        .collect()
}

/// Parse a single nftables rule line, extracting comment as description.
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
