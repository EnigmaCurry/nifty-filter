use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_ddns))
}

#[derive(Serialize, JsonSchema)]
struct DdnsResponse {
    records: Vec<DdnsRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct DdnsRecord {
    domain: String,
    owner: String,
    provider: String,
    ip_version: String,
    status: String,
    status_class: String,
    current_ip: String,
    previous_ips: String,
}

#[api_doc(
    id = "get_ddns",
    tag = "ddns",
    ok = "Json<ApiResponse<DdnsResponse>>",
    err = "Json<ErrorBody>"
)]
/// DDNS updater status
///
/// Returns the current status of DDNS records from the ddns-updater service
/// running on the services host. Only returns data when DDNS is configured.
async fn get_ddns(state: State<AppState>) -> ApiJson<DdnsResponse> {
    match fetch_ddns_data(&state.services_client).await {
        Ok(resp) => json_ok(resp),
        Err(msg) => json_ok(DdnsResponse {
            records: vec![],
            error: Some(msg),
        }),
    }
}

fn read_ddns_config() -> Result<DdnsServiceInfo, String> {
    let path = crate::config_watcher::config_file_path();
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read config: {e}"))?;

    let config: serde_json::Value =
        hcl::from_str(&contents).map_err(|e| format!("HCL parse error: {e}"))?;

    let services = config
        .get("services")
        .ok_or("no services block in config")?;

    // Check that ddns is configured
    let ddns = services.get("ddns").ok_or("ddns not configured")?;
    if ddns.is_null() {
        return Err("ddns not configured".to_string());
    }

    let host = services.get("host");

    let domain = host
        .and_then(|h| h.get("domain"))
        .and_then(|v| v.as_str())
        .unwrap_or("nifty.internal")
        .to_string();

    Ok(DdnsServiceInfo {
        domain,
    })
}

struct DdnsServiceInfo {
    domain: String,
}

async fn fetch_ddns_data(services_client: &reqwest::Client) -> Result<DdnsResponse, String> {
    let info = read_ddns_config()?;

    let ddns_host = format!("ddns.{}", info.domain);
    let base_url = format!("https://{ddns_host}");

    let resp = services_client
        .get(&base_url)
        .send()
        .await
        .map_err(|e| format!("ddns-updater request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("ddns-updater returned HTTP {}", resp.status()));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;

    let records = parse_ddns_html(&html);

    Ok(DdnsResponse {
        records,
        error: None,
    })
}

/// Parse the ddns-updater HTML table into structured records.
fn parse_ddns_html(html: &str) -> Vec<DdnsRecord> {
    let mut records = Vec::new();

    // Match each <tr> in <tbody>
    let tbody_re = Regex::new(r"(?s)<tbody>(.*?)</tbody>").unwrap();
    let tr_re = Regex::new(r"(?s)<tr>(.*?)</tr>").unwrap();
    let td_re = Regex::new(r#"(?s)<td[^>]*data-label="([^"]*)"[^>]*>(.*?)</td>"#).unwrap();
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let class_re = Regex::new(r#"class="([^"]*)""#).unwrap();

    let tbody = match tbody_re.captures(html) {
        Some(cap) => cap[1].to_string(),
        None => return records,
    };

    for tr_cap in tr_re.captures_iter(&tbody) {
        let tr_html = &tr_cap[1];
        let mut domain = String::new();
        let mut owner = String::new();
        let mut provider = String::new();
        let mut ip_version = String::new();
        let mut status = String::new();
        let mut status_class = String::new();
        let mut current_ip = String::new();
        let mut previous_ips = String::new();

        for td_cap in td_re.captures_iter(tr_html) {
            let label = &td_cap[1];
            let content = &td_cap[2];
            let text = tag_re.replace_all(content, "").trim().to_string();

            match label {
                "Domain" => domain = text,
                "Owner" => owner = text,
                "Provider" => provider = text,
                "IP Version" => ip_version = text,
                "Update Status" => {
                    status = text;
                    // Extract the CSS class for status coloring
                    if let Some(cls) = class_re.captures(content) {
                        status_class = cls[1].to_string();
                    }
                }
                "Current IP" => current_ip = text,
                "Previous IPs" => previous_ips = text,
                _ => {}
            }
        }

        if !domain.is_empty() {
            records.push(DdnsRecord {
                domain,
                owner,
                provider,
                ip_version,
                status,
                status_class,
                current_ip,
                previous_ips,
            });
        }
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real HTML captured from a running ddns-updater instance (single record).
    const SAMPLE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head><title>DDNS Updater</title></head>
<body>
  <table role="table">
    <thead>
      <tr>
        <th>Domain</th><th>Owner</th><th>Provider</th>
        <th>IP Version</th><th>Update Status</th><th>Current IP</th>
        <th>Previous IPs<small> (reverse chronological order)</small></th>
      </tr>
    </thead>
    <tbody>
      <tr>
        <td data-label="Domain"><a href="http://tellarite.duckdns.org">tellarite.duckdns.org</a></td>
        <td data-label="Owner">@</td>
        <td data-label="Provider"><a href="https://www.duckdns.org/">DuckDNS</a></td>
        <td data-label="IP Version">ipv4 or ipv6</td>
        <td data-label="Update Status"><span class="success">Success</span> (changed to 24.2.66.11), 7m43s ago</td>
        <td data-label="Current IP"><a href="https://ipinfo.io/24.2.66.11">24.2.66.11</a></td>
        <td data-label="Previous IPs">N/A</td>
      </tr>
    </tbody>
  </table>
</body>
</html>"#;

    #[test]
    fn parse_single_record() {
        let records = parse_ddns_html(SAMPLE_HTML);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.domain, "tellarite.duckdns.org");
        assert_eq!(r.owner, "@");
        assert_eq!(r.provider, "DuckDNS");
        assert_eq!(r.ip_version, "ipv4 or ipv6");
        assert!(r.status.contains("Success"));
        assert!(r.status.contains("7m43s ago"));
        assert_eq!(r.status_class, "success");
        assert_eq!(r.current_ip, "24.2.66.11");
        assert_eq!(r.previous_ips, "N/A");
    }

    #[test]
    fn parse_multiple_records() {
        let html = r#"<tbody>
      <tr>
        <td data-label="Domain"><a href="http://a.example.com">a.example.com</a></td>
        <td data-label="Owner">@</td>
        <td data-label="Provider">Cloudflare</td>
        <td data-label="IP Version">ipv4</td>
        <td data-label="Update Status"><span class="success">Success</span> (changed to 1.2.3.4), 2m ago</td>
        <td data-label="Current IP">1.2.3.4</td>
        <td data-label="Previous IPs">5.6.7.8</td>
      </tr>
      <tr>
        <td data-label="Domain"><a href="http://b.example.com">b.example.com</a></td>
        <td data-label="Owner">sub</td>
        <td data-label="Provider">DuckDNS</td>
        <td data-label="IP Version">ipv6</td>
        <td data-label="Update Status"><span class="error">Error</span> token expired</td>
        <td data-label="Current IP">-</td>
        <td data-label="Previous IPs">N/A</td>
      </tr>
    </tbody>"#;

        let records = parse_ddns_html(html);
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].domain, "a.example.com");
        assert_eq!(records[0].provider, "Cloudflare");
        assert_eq!(records[0].current_ip, "1.2.3.4");
        assert_eq!(records[0].previous_ips, "5.6.7.8");
        assert_eq!(records[0].status_class, "success");

        assert_eq!(records[1].domain, "b.example.com");
        assert_eq!(records[1].owner, "sub");
        assert_eq!(records[1].ip_version, "ipv6");
        assert_eq!(records[1].status_class, "error");
    }

    #[test]
    fn parse_empty_tbody() {
        let html = "<tbody>\n    </tbody>";
        let records = parse_ddns_html(html);
        assert!(records.is_empty());
    }

    #[test]
    fn parse_no_tbody() {
        let html = "<html><body>no table here</body></html>";
        let records = parse_ddns_html(html);
        assert!(records.is_empty());
    }
}
