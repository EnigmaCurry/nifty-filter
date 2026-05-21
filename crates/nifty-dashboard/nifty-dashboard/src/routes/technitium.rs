use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_technitium))
}

// --- Response types ---

#[derive(Serialize, JsonSchema)]
struct TechnitiumResponse {
    /// Forwarder settings from Technitium
    forwarders: Option<ForwarderInfo>,
    /// DNS zones managed by Technitium
    zones: Vec<ZoneInfo>,
    /// Error message if the data could not be fetched
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct ForwarderInfo {
    forwarders: Vec<String>,
    forwarder_protocol: String,
    #[serde(rename = "forwarderConcurrency")]
    forwarder_concurrency: u32,
    #[serde(rename = "concurrentForwarding")]
    concurrent_forwarding: bool,
}

#[derive(Serialize, JsonSchema)]
struct ZoneInfo {
    name: String,
    #[serde(rename = "type")]
    zone_type: String,
    records: Vec<RecordInfo>,
}

#[derive(Serialize, JsonSchema)]
struct RecordInfo {
    name: String,
    #[serde(rename = "type")]
    record_type: String,
    ttl: u32,
    value: String,
}

// --- Technitium API response types ---

#[derive(Deserialize)]
struct TechLoginResponse {
    status: String,
    token: Option<String>,
}


#[derive(Deserialize)]
struct TechZoneListResponse {
    status: String,
    response: Option<TechZoneListData>,
}

#[derive(Deserialize)]
struct TechZoneListData {
    #[serde(default)]
    zones: Vec<TechZoneEntry>,
}

#[derive(Deserialize)]
struct TechZoneEntry {
    name: String,
    #[serde(rename = "type")]
    zone_type: String,
}

#[derive(Deserialize)]
struct TechRecordListResponse {
    status: String,
    response: Option<TechRecordListData>,
}

#[derive(Deserialize)]
struct TechRecordListData {
    #[serde(default)]
    records: Vec<TechRecordEntry>,
}

#[derive(Deserialize)]
struct TechRecordEntry {
    name: String,
    #[serde(rename = "type")]
    record_type: String,
    ttl: u32,
    #[serde(rename = "rData")]
    rdata: serde_json::Value,
}

// --- Handler ---

#[api_doc(
    id = "get_technitium",
    tag = "technitium",
    ok = "Json<ApiResponse<TechnitiumResponse>>",
    err = "Json<ErrorBody>"
)]
/// Technitium DNS status
///
/// Returns forwarder configuration and zone information from the Technitium DNS
/// server running on the services host. Authenticates as the viewer user.
async fn get_technitium(state: State<AppState>) -> ApiJson<TechnitiumResponse> {
    match fetch_technitium_data(&state.services_client).await {
        Ok(resp) => json_ok(resp),
        Err(msg) => json_ok(TechnitiumResponse {
            forwarders: None,
            zones: vec![],
            error: Some(msg),
        }),
    }
}

struct ServicesInfo {
    ip_address: String,
    domain: String,
    viewer_password: String,
    forwarders: Vec<String>,
    forwarder_protocol: String,
    forwarder_concurrency: u32,
}

/// Read HCL config and extract services.host.ip_address and services.dns.viewer_password.
fn read_services_config() -> Result<ServicesInfo, String> {
    let path = crate::config_watcher::config_file_path();
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config: {e}"))?;

    let config: serde_json::Value = hcl::from_str(&contents)
        .map_err(|e| format!("HCL parse error: {e}"))?;

    let services = config
        .get("services")
        .ok_or("no services block in config")?;

    let host = services.get("host");

    let ip_address = host
        .and_then(|h| h.get("ip_address"))
        .and_then(|v| v.as_str())
        .ok_or("services.host.ip_address not configured")?
        .to_string();

    let domain = host
        .and_then(|h| h.get("domain"))
        .and_then(|v| v.as_str())
        .unwrap_or("nifty.internal")
        .to_string();

    let dns = services.get("dns");

    let viewer_password = dns
        .and_then(|d| d.get("viewer_password"))
        .and_then(|v| v.as_str())
        .ok_or("services.dns.viewer_password not configured")?
        .to_string();

    let forwarders = dns
        .and_then(|d| d.get("forwarders"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let forwarder_protocol = dns
        .and_then(|d| d.get("forwarder_protocol"))
        .and_then(|v| v.as_str())
        .unwrap_or("tls")
        .to_string();

    let forwarder_concurrency = dns
        .and_then(|d| d.get("forwarder_concurrency"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as u32;

    Ok(ServicesInfo {
        ip_address,
        domain,
        viewer_password,
        forwarders,
        forwarder_protocol,
        forwarder_concurrency,
    })
}

async fn fetch_technitium_data(services_client: &reqwest::Client) -> Result<TechnitiumResponse, String> {
    let info = read_services_config()?;

    let dns_host = format!("dns.{}", info.domain);
    let base_url = format!("https://{}", info.ip_address);

    let client = services_client;

    // Forwarder info comes from the HCL config (settings API requires admin).
    let concurrent = info.forwarders.len() > 1;
    let forwarders = Some(ForwarderInfo {
        forwarders: info.forwarders,
        forwarder_protocol: info.forwarder_protocol,
        forwarder_concurrency: info.forwarder_concurrency,
        concurrent_forwarding: concurrent,
    });

    // Login as viewer and fetch zones (viewer has per-zone read access).
    let token = login(client, &base_url, &dns_host, &info.viewer_password).await?;
    let zones = fetch_zones(client, &base_url, &dns_host, &token).await.unwrap_or_default();

    Ok(TechnitiumResponse {
        forwarders,
        zones,
        error: None,
    })
}

/// Parse a JSON response body, returning a descriptive error on failure.
async fn parse_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    context: &str,
) -> Result<T, String> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("{context}: failed to read response body: {e}"))?;

    if !status.is_success() {
        let preview: String = body.chars().take(200).collect();
        return Err(format!("{context}: HTTP {status}: {preview}"));
    }

    serde_json::from_str(&body).map_err(|e| {
        let preview: String = body.chars().take(200).collect();
        format!("{context}: failed to parse JSON: {e} (body: {preview})")
    })
}

async fn login(
    client: &reqwest::Client,
    base_url: &str,
    host: &str,
    password: &str,
) -> Result<String, String> {
    let url = format!("{base_url}/api/user/login");
    let resp = client
        .post(&url)
        .header(reqwest::header::HOST, host)
        .form(&[("user", "viewer"), ("pass", password)])
        .send()
        .await
        .map_err(|e| format!("login request failed: {e}"))?;

    let body: TechLoginResponse = parse_json(resp, "login").await?;

    if body.status == "ok" {
        body.token
            .ok_or_else(|| "login succeeded but no token returned".to_string())
    } else {
        Err(format!("login failed (status: {})", body.status))
    }
}


async fn fetch_zones(
    client: &reqwest::Client,
    base_url: &str,
    host: &str,
    token: &str,
) -> Result<Vec<ZoneInfo>, String> {
    let url = format!("{base_url}/api/zones/list");
    let resp = client
        .get(&url)
        .header(reqwest::header::HOST, host)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("zone list request failed: {e}"))?;

    let body: TechZoneListResponse = parse_json(resp, "zone list").await?;

    if body.status != "ok" {
        return Err(format!("zone list failed (status: {})", body.status));
    }

    let zone_entries = body
        .response
        .map(|r| r.zones)
        .unwrap_or_default();

    let mut zones = Vec::new();
    for entry in zone_entries {
        let records = fetch_zone_records(client, base_url, host, token, &entry.name)
            .await
            .unwrap_or_default();
        zones.push(ZoneInfo {
            name: entry.name,
            zone_type: entry.zone_type,
            records,
        });
    }

    Ok(zones)
}

async fn fetch_zone_records(
    client: &reqwest::Client,
    base_url: &str,
    host: &str,
    token: &str,
    zone: &str,
) -> Result<Vec<RecordInfo>, String> {
    let url = format!("{base_url}/api/zones/records/get");
    let resp = client
        .get(&url)
        .header(reqwest::header::HOST, host)
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("domain", zone), ("zone", zone), ("listZone", "true")])
        .send()
        .await
        .map_err(|e| format!("record list request failed: {e}"))?;

    let body: TechRecordListResponse = parse_json(resp, "record list").await?;

    if body.status != "ok" {
        return Err(format!("record list failed (status: {})", body.status));
    }

    let records = body
        .response
        .map(|r| r.records)
        .unwrap_or_default();

    Ok(records
        .into_iter()
        .map(|r| RecordInfo {
            name: r.name,
            record_type: r.record_type.clone(),
            ttl: r.ttl,
            value: format_rdata(&r.record_type, &r.rdata),
        })
        .collect())
}

/// Format rdata into a human-readable value string.
fn format_rdata(record_type: &str, rdata: &serde_json::Value) -> String {
    match record_type {
        "A" | "AAAA" => rdata
            .get("ipAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string(),
        "CNAME" => rdata
            .get("cname")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string(),
        "NS" => rdata
            .get("nameServer")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string(),
        "TXT" => rdata
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string(),
        "MX" => {
            let exchange = rdata.get("exchange").and_then(|v| v.as_str()).unwrap_or("-");
            let pref = rdata.get("preference").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{pref} {exchange}")
        }
        "SRV" => {
            let target = rdata.get("target").and_then(|v| v.as_str()).unwrap_or("-");
            let port = rdata.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
            let priority = rdata.get("priority").and_then(|v| v.as_u64()).unwrap_or(0);
            let weight = rdata.get("weight").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{priority} {weight} {port} {target}")
        }
        "CAA" => {
            let flags = rdata.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
            let tag = rdata.get("tag").and_then(|v| v.as_str()).unwrap_or("-");
            let value = rdata.get("value").and_then(|v| v.as_str()).unwrap_or("-");
            format!("{flags} {tag} \"{value}\"")
        }
        "SOA" => {
            let pns = rdata.get("primaryNameServer").and_then(|v| v.as_str()).unwrap_or("-");
            let admin = rdata.get("responsiblePerson").and_then(|v| v.as_str()).unwrap_or("-");
            let serial = rdata.get("serial").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{pns} {admin} {serial}")
        }
        _ => serde_json::to_string(rdata).unwrap_or_else(|_| "-".to_string()),
    }
}
