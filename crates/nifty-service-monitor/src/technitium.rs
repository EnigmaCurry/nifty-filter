use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use log::{debug, error, info, warn};
use reqwest::Client;
use serde::Deserialize;

use crate::config::{DnsServiceConfig, ZoneConfig};

const BASE_URL: &str = "http://localhost:5380";
const DEFAULT_USER: &str = "admin";
const VIEWER_USER: &str = "viewer";
const ADMIN_PASSWORD_FILE: &str = "technitium-admin-password";

/// Check if a zone is a Technitium system zone that should never be deleted.
fn is_system_zone(name: &str) -> bool {
    name == "localhost"
        || name.ends_with(".in-addr.arpa")
        || name.ends_with(".ip6.arpa")
}

#[derive(Deserialize)]
struct LoginResponse {
    status: String,
    token: Option<String>,
}

#[derive(Deserialize)]
struct TechnitiumApiResponse {
    status: String,
}

#[derive(Deserialize)]
struct ZoneListResponse {
    status: String,
    response: Option<ZoneListData>,
}

#[derive(Deserialize)]
struct ZoneListData {
    #[serde(default)]
    zones: Vec<ZoneEntry>,
}

#[derive(Deserialize)]
struct ZoneEntry {
    name: String,
}

#[derive(Deserialize)]
struct RecordListResponse {
    status: String,
    response: Option<RecordListData>,
}

#[derive(Deserialize)]
struct RecordListData {
    #[serde(default)]
    records: Vec<RecordEntry>,
}

#[derive(Deserialize)]
struct RecordEntry {
    name: String,
    #[serde(rename = "type")]
    record_type: String,
    #[serde(rename = "rData")]
    rdata: serde_json::Value,
}

fn admin_password_path(state_dir: &Path) -> PathBuf {
    state_dir.join(ADMIN_PASSWORD_FILE)
}

/// Read the admin password from the state directory, or None if not yet persisted.
fn read_admin_password(state_dir: &Path) -> Option<String> {
    std::fs::read_to_string(admin_password_path(state_dir)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}


enum LoginError {
    /// Technitium is not reachable (not started yet, network issue, etc.)
    Unavailable(String),
    /// Authentication failed or other API-level error.
    Auth(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::Unavailable(msg) | LoginError::Auth(msg) => f.write_str(msg),
        }
    }
}

/// Attempt to log in to Technitium, returning the session token on success.
async fn try_login(client: &Client, password: &str) -> Result<String, LoginError> {
    let url = format!("{}/api/user/login", BASE_URL);
    let resp = client
        .post(&url)
        .form(&[("user", DEFAULT_USER), ("pass", password)])
        .send()
        .await
        .map_err(|e| LoginError::Unavailable(format!("login request failed: {e}")))?;

    let body: LoginResponse = resp
        .json()
        .await
        .map_err(|e| LoginError::Auth(format!("failed to parse login response: {e}")))?;

    if body.status == "ok" {
        body.token
            .ok_or_else(|| LoginError::Auth("login succeeded but no token returned".to_string()))
    } else {
        Err(LoginError::Auth(format!("login failed (status: {})", body.status)))
    }
}


/// Create or update a user account.
async fn ensure_user(
    client: &Client,
    token: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    // Try to create the user first.
    let url = format!("{}/api/admin/users/create", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("user", username), ("pass", password)])
        .send()
        .await
        .map_err(|e| format!("user create request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse user create response: {e}"))?;

    if body.status == "ok" {
        return Ok(());
    }

    // User likely already exists — update password.
    let url = format!("{}/api/admin/users/set", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("user", username), ("newPass", password)])
        .send()
        .await
        .map_err(|e| format!("user set request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse user set response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("user set failed (status: {})", body.status))
    }
}

/// Get the set of existing zone names.
async fn list_zones(client: &Client, token: &str) -> Result<HashSet<String>, String> {
    let url = format!("{}/api/zones/list", BASE_URL);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("zone list request failed: {e}"))?;

    let body: ZoneListResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone list response: {e}"))?;

    if body.status != "ok" {
        return Err(format!("zone list failed (status: {})", body.status));
    }

    Ok(body
        .response
        .map(|r| r.zones.into_iter().map(|z| z.name).collect())
        .unwrap_or_default())
}

/// Create a primary zone.
async fn create_zone(client: &Client, token: &str, zone: &str) -> Result<(), String> {
    let url = format!("{}/api/zones/create", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("zone", zone), ("type", "Primary")])
        .send()
        .await
        .map_err(|e| format!("zone create request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone create response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("zone create failed (status: {})", body.status))
    }
}

/// Delete a zone.
async fn delete_zone(client: &Client, token: &str, zone: &str) -> Result<(), String> {
    let url = format!("{}/api/zones/delete", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("zone", zone)])
        .send()
        .await
        .map_err(|e| format!("zone delete request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone delete response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("zone delete failed (status: {})", body.status))
    }
}

/// List all records in a zone.
async fn list_records(client: &Client, token: &str, zone: &str) -> Result<Vec<RecordEntry>, String> {
    let url = format!("{}/api/zones/records/get", BASE_URL);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("domain", zone), ("zone", zone), ("listZone", "true")])
        .send()
        .await
        .map_err(|e| format!("record list request failed: {e}"))?;

    let body: RecordListResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse record list response: {e}"))?;

    if body.status != "ok" {
        return Err(format!("record list failed (status: {})", body.status));
    }

    Ok(body.response.map(|r| r.records).unwrap_or_default())
}

/// Delete a specific DNS record.
async fn delete_record(
    client: &Client,
    token: &str,
    domain: &str,
    record_type: &str,
    rdata: &serde_json::Value,
) -> Result<(), String> {
    let url = format!("{}/api/zones/records/delete", BASE_URL);
    let mut params = vec![
        ("domain".to_string(), domain.to_string()),
        ("type".to_string(), record_type.to_string()),
    ];

    match record_type {
        "A" | "AAAA" => {
            if let Some(ip) = rdata.get("ipAddress").and_then(|v| v.as_str()) {
                params.push(("ipAddress".into(), ip.into()));
            }
        }
        "CNAME" => {
            if let Some(cname) = rdata.get("cname").and_then(|v| v.as_str()) {
                params.push(("cname".into(), cname.into()));
            }
        }
        "NS" => {
            if let Some(ns) = rdata.get("nameServer").and_then(|v| v.as_str()) {
                params.push(("nameServer".into(), ns.into()));
            }
        }
        "MX" => {
            if let Some(ex) = rdata.get("exchange").and_then(|v| v.as_str()) {
                params.push(("exchange".into(), ex.into()));
            }
        }
        "TXT" => {
            if let Some(text) = rdata.get("text").and_then(|v| v.as_str()) {
                params.push(("text".into(), text.into()));
            }
        }
        "SRV" => {
            if let Some(t) = rdata.get("target").and_then(|v| v.as_str()) {
                params.push(("srvTarget".into(), t.into()));
            }
            if let Some(p) = rdata.get("port") {
                params.push(("srvPort".into(), p.to_string()));
            }
        }
        "CAA" => {
            if let Some(tag) = rdata.get("tag").and_then(|v| v.as_str()) {
                params.push(("tag".into(), tag.into()));
            }
            if let Some(val) = rdata.get("value").and_then(|v| v.as_str()) {
                params.push(("value".into(), val.into()));
            }
        }
        _ => {}
    }

    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&params_ref)
        .send()
        .await
        .map_err(|e| format!("record delete request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse record delete response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("record delete failed (status: {})", body.status))
    }
}

/// Add or overwrite a DNS record via the Technitium API.
async fn add_record(
    client: &Client,
    token: &str,
    params: &[(&str, String)],
) -> Result<(), String> {
    let url = format!("{}/api/zones/records/add", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("record add request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse record add response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("record add failed (status: {})", body.status))
    }
}

/// Resolve a hostname key to an FQDN.
/// "@" means the zone apex (returns the zone name itself).
fn to_fqdn(hostname: &str, zone: &str) -> String {
    if hostname == "@" {
        zone.to_string()
    } else {
        format!("{hostname}.{zone}")
    }
}

/// Extract the rdata value string for a simple record type from the API response.
fn rdata_value(record_type: &str, rdata: &serde_json::Value) -> Option<String> {
    match record_type {
        "A" | "AAAA" => rdata.get("ipAddress").and_then(|v| v.as_str()).map(|s| s.to_string()),
        "CNAME" => rdata.get("cname").and_then(|v| v.as_str()).map(|s| s.to_string()),
        "NS" => rdata.get("nameServer").and_then(|v| v.as_str()).map(|s| s.to_string()),
        "TXT" => rdata.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None,
    }
}

/// Build lookup of existing records: (fqdn, type) -> rdata value string.
/// Only covers simple types (A, AAAA, CNAME, NS, TXT) for diff comparison.
/// Complex types (MX, SRV, CAA) are always re-applied.
fn build_existing_map(records: &[RecordEntry]) -> HashMap<(String, String), String> {
    let mut map = HashMap::new();
    for r in records {
        if let Some(val) = rdata_value(&r.record_type, &r.rdata) {
            map.insert((r.name.clone(), r.record_type.clone()), val);
        }
    }
    map
}

/// Build the set of expected (fqdn, type) pairs from the zone config.
fn expected_records(zone_name: &str, zone: &ZoneConfig) -> HashSet<(String, String)> {
    let mut expected = HashSet::new();
    for host in zone.A.keys() {
        expected.insert((to_fqdn(host, zone_name), "A".into()));
    }
    for host in zone.AAAA.keys() {
        expected.insert((to_fqdn(host, zone_name), "AAAA".into()));
    }
    for host in zone.CNAME.keys() {
        expected.insert((to_fqdn(host, zone_name), "CNAME".into()));
    }
    for host in zone.NS.keys() {
        expected.insert((to_fqdn(host, zone_name), "NS".into()));
    }
    for host in zone.TXT.keys() {
        expected.insert((to_fqdn(host, zone_name), "TXT".into()));
    }
    for host in zone.MX.keys() {
        expected.insert((to_fqdn(host, zone_name), "MX".into()));
    }
    for host in zone.SRV.keys() {
        expected.insert((to_fqdn(host, zone_name), "SRV".into()));
    }
    for host in zone.CAA.keys() {
        expected.insert((to_fqdn(host, zone_name), "CAA".into()));
    }
    expected
}

/// Reconcile a zone: add missing/changed records, delete stale ones.
async fn reconcile_zone(
    client: &Client,
    token: &str,
    zone_name: &str,
    zone: &ZoneConfig,
) -> bool {
    let existing = match list_records(client, token, zone_name).await {
        Ok(r) => r,
        Err(e) => {
            warn!("technitium: failed to list records for '{zone_name}': {e}");
            return false;
        }
    };

    let existing_map = build_existing_map(&existing);
    let expected = expected_records(zone_name, zone);
    let mut all_ok = true;
    let ttl = "3600".to_string();

    // --- Add missing or changed records ---

    for (host, addr) in &zone.A {
        let fqdn = to_fqdn(host, zone_name);
        let key = (fqdn.clone(), "A".to_string());
        if existing_map.get(&key).map(|s| s.as_str()) == Some(addr.as_str()) {
            continue;
        }
        let params = vec![
            ("domain", fqdn.clone()), ("type", "A".into()),
            ("overwrite", "true".into()), ("ttl", ttl.clone()),
            ("ipAddress", addr.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => info!("technitium: A {fqdn} -> {addr}"),
            Err(e) => { error!("technitium: failed to add A {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, addr) in &zone.AAAA {
        let fqdn = to_fqdn(host, zone_name);
        let key = (fqdn.clone(), "AAAA".to_string());
        if existing_map.get(&key).map(|s| s.as_str()) == Some(addr.as_str()) {
            continue;
        }
        let params = vec![
            ("domain", fqdn.clone()), ("type", "AAAA".into()),
            ("overwrite", "true".into()), ("ttl", ttl.clone()),
            ("ipAddress", addr.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => info!("technitium: AAAA {fqdn} -> {addr}"),
            Err(e) => { error!("technitium: failed to add AAAA {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, target) in &zone.CNAME {
        let fqdn = to_fqdn(host, zone_name);
        let key = (fqdn.clone(), "CNAME".to_string());
        if existing_map.get(&key).map(|s| s.as_str()) == Some(target.as_str()) {
            continue;
        }
        let params = vec![
            ("domain", fqdn.clone()), ("type", "CNAME".into()),
            ("overwrite", "true".into()), ("ttl", ttl.clone()),
            ("cname", target.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => info!("technitium: CNAME {fqdn} -> {target}"),
            Err(e) => { error!("technitium: failed to add CNAME {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, target) in &zone.NS {
        let fqdn = to_fqdn(host, zone_name);
        let key = (fqdn.clone(), "NS".to_string());
        if existing_map.get(&key).map(|s| s.as_str()) == Some(target.as_str()) {
            continue;
        }
        let params = vec![
            ("domain", fqdn.clone()), ("type", "NS".into()),
            ("overwrite", "true".into()), ("ttl", ttl.clone()),
            ("nameServer", target.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => info!("technitium: NS {fqdn} -> {target}"),
            Err(e) => { error!("technitium: failed to add NS {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, text) in &zone.TXT {
        let fqdn = to_fqdn(host, zone_name);
        let key = (fqdn.clone(), "TXT".to_string());
        if existing_map.get(&key).map(|s| s.as_str()) == Some(text.as_str()) {
            continue;
        }
        let params = vec![
            ("domain", fqdn.clone()), ("type", "TXT".into()),
            ("overwrite", "true".into()), ("ttl", ttl.clone()),
            ("text", text.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => info!("technitium: TXT {fqdn}"),
            Err(e) => { error!("technitium: failed to add TXT {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, mx) in &zone.MX {
        let fqdn = to_fqdn(host, zone_name);
        let record_ttl = mx.ttl.unwrap_or(3600).to_string();
        let params = vec![
            ("domain", fqdn.clone()), ("type", "MX".into()),
            ("overwrite", "true".into()), ("ttl", record_ttl),
            ("exchange", mx.exchange.clone()),
            ("preference", mx.preference.to_string()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => debug!("technitium: MX {fqdn} -> {}", mx.exchange),
            Err(e) => { error!("technitium: failed to add MX {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, srv) in &zone.SRV {
        let fqdn = to_fqdn(host, zone_name);
        let record_ttl = srv.ttl.unwrap_or(3600).to_string();
        let params = vec![
            ("domain", fqdn.clone()), ("type", "SRV".into()),
            ("overwrite", "true".into()), ("ttl", record_ttl),
            ("srvTarget", srv.target.clone()),
            ("srvPort", srv.port.to_string()),
            ("srvPriority", srv.priority.to_string()),
            ("srvWeight", srv.weight.to_string()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => debug!("technitium: SRV {fqdn} -> {}:{}", srv.target, srv.port),
            Err(e) => { error!("technitium: failed to add SRV {fqdn}: {e}"); all_ok = false; }
        }
    }

    for (host, caa) in &zone.CAA {
        let fqdn = to_fqdn(host, zone_name);
        let record_ttl = caa.ttl.unwrap_or(3600).to_string();
        let params = vec![
            ("domain", fqdn.clone()), ("type", "CAA".into()),
            ("overwrite", "true".into()), ("ttl", record_ttl),
            ("flags", caa.flags.to_string()),
            ("tag", caa.tag.clone()),
            ("value", caa.value.clone()),
        ];
        match add_record(client, token, &params).await {
            Ok(()) => debug!("technitium: CAA {fqdn} {} {}", caa.tag, caa.value),
            Err(e) => { error!("technitium: failed to add CAA {fqdn}: {e}"); all_ok = false; }
        }
    }

    // --- Delete stale records ---

    for record in &existing {
        if record.record_type == "SOA" {
            continue;
        }
        if record.record_type == "NS" && record.name.eq_ignore_ascii_case(zone_name) {
            continue;
        }

        let key = (record.name.clone(), record.record_type.clone());
        if !expected.contains(&key) {
            info!(
                "technitium: deleting stale {} record '{}'",
                record.record_type, record.name
            );
            if let Err(e) = delete_record(
                client, token, &record.name, &record.record_type, &record.rdata,
            ).await {
                error!(
                    "technitium: failed to delete {} '{}': {e}",
                    record.record_type, record.name
                );
                all_ok = false;
            }
        }
    }

    all_ok
}

/// Grant view-only permission on a zone to the viewer user.
async fn grant_zone_view(
    client: &Client,
    token: &str,
    zone: &str,
) -> Result<(), String> {
    let url = format!("{}/api/zones/permissions/set", BASE_URL);
    // Pipe-separated: username|canView|canModify|canDelete
    let user_perms = format!("admin|true|true|true|{VIEWER_USER}|true|false|false");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("zone", zone), ("userPermissions", user_perms.as_str())])
        .send()
        .await
        .map_err(|e| format!("zone permissions request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone permissions response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("zone permissions set failed (status: {})", body.status))
    }
}

/// Set DNS forwarders via the Technitium settings API.
/// Set the DNS server's primary domain name.
async fn set_domain(
    client: &Client,
    token: &str,
    domain: &str,
) -> Result<(), String> {
    let url = format!("{}/api/settings/set", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("dnsServerDomain", domain)])
        .send()
        .await
        .map_err(|e| format!("set domain request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse set domain response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("set domain failed (status: {})", body.status))
    }
}

async fn set_forwarders(
    client: &Client,
    token: &str,
    config: &DnsServiceConfig,
) -> Result<(), String> {
    let forwarders = config.forwarders.join(",");
    let protocol_raw = config
        .forwarder_protocol
        .as_deref()
        .unwrap_or("tls");
    let protocol = match protocol_raw.to_lowercase().as_str() {
        "udp" => "Udp",
        "tcp" => "Tcp",
        "tls" => "Tls",
        "https" => "Https",
        "quic" => "Quic",
        other => {
            return Err(format!("unknown forwarder_protocol: {other}"));
        }
    };
    let concurrency = config.forwarder_concurrency.unwrap_or(2);
    let enable_concurrency = config.forwarders.len() > 1;

    let url = format!("{}/api/settings/set", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[
            ("forwarders", forwarders.as_str()),
            ("forwarderProtocol", protocol),
            ("concurrentForwarding", if enable_concurrency { "true" } else { "false" }),
            ("forwarderConcurrency", &concurrency.to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("set forwarders request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse set forwarders response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("set forwarders failed (status: {})", body.status))
    }
}

/// Tracks state across poll cycles.
pub struct TechnitiumState {
    state_dir: PathBuf,
    /// The admin password currently in use (read from state_dir or generated).
    admin_password: Option<String>,
}

impl TechnitiumState {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            admin_password: read_admin_password(state_dir),
            state_dir: state_dir.to_path_buf(),
        }
    }
}

/// Log in as admin using the pre-seeded password from the state directory.
///
/// The admin password file is generated by the technitium-admin-password
/// systemd service and shared between the Technitium container (via
/// DNS_SERVER_ADMIN_PASSWORD_FILE) and this service monitor via the
/// service-monitor-data volume.
async fn admin_login(
    client: &Client,
    state: &mut TechnitiumState,
) -> Result<String, String> {
    // Load password from state dir if not cached yet.
    if state.admin_password.is_none() {
        state.admin_password = read_admin_password(&state.state_dir);
    }

    let pw = state.admin_password.as_ref().ok_or_else(|| {
        format!(
            "admin password file not found at {}",
            admin_password_path(&state.state_dir).display()
        )
    })?;

    match try_login(client, pw).await {
        Ok(token) => Ok(token),
        Err(LoginError::Unavailable(e)) => {
            debug!("technitium: not reachable yet: {e}");
            Err("not reachable".into())
        }
        Err(LoginError::Auth(e)) => {
            Err(format!("admin login failed: {e}"))
        }
    }
}

/// Apply all Technitium configuration declaratively.
/// Returns true if the cycle completed successfully, false on any error.
/// A "not reachable" result (Technitium not started yet) returns true
/// since that's expected during startup, not a persistent failure.
pub async fn apply(client: &Client, config: &DnsServiceConfig, domain: &str, state: &mut TechnitiumState) -> bool {
    // Log in as admin.
    let token = match admin_login(client, state).await {
        Ok(t) => t,
        Err(ref e) if e == "not reachable" => {
            debug!("technitium: waiting for server to become available");
            return true; // expected during startup
        }
        Err(e) => {
            warn!("technitium: {e}");
            return false;
        }
    };

    // Manage viewer user.
    if let Some(ref viewer_pw) = config.viewer_password {
        if !viewer_pw.is_empty() {
            match ensure_user(client, &token, VIEWER_USER, viewer_pw).await {
                Ok(()) => debug!("technitium: viewer user configured"),
                Err(e) => error!("technitium: failed to configure viewer user: {e}"),
            }
        }
    }

    // Set DNS server domain.
    let dns_domain = format!("dns.{domain}");
    match set_domain(client, &token, &dns_domain).await {
        Ok(()) => debug!("technitium: domain set to {dns_domain}"),
        Err(e) => error!("technitium: failed to set domain: {e}"),
    }

    // Apply forwarders.
    if !config.forwarders.is_empty() {
        match set_forwarders(client, &token, config).await {
            Ok(()) => {
                debug!(
                    "technitium: forwarders applied: {} (protocol: {})",
                    config.forwarders.join(", "),
                    config.forwarder_protocol.as_deref().unwrap_or("tls")
                );
            }
            Err(e) => {
                error!("technitium: failed to set forwarders: {e}");
            }
        }
    }

    // Fetch existing zones.
    let existing_zones = match list_zones(client, &token).await {
        Ok(z) => z,
        Err(e) => {
            warn!("technitium: failed to list zones: {e}");
            return false;
        }
    };

    let declared_zones: HashSet<&str> = config.zone.keys().map(|s| s.as_str()).collect();
    let unmanaged: HashSet<&str> = config.unmanaged_zones.iter().map(|s| s.as_str()).collect();

    // Delete zones not in config, not system, not unmanaged.
    for zone_name in &existing_zones {
        if is_system_zone(zone_name) {
            continue;
        }
        if declared_zones.contains(zone_name.as_str()) {
            continue;
        }
        if unmanaged.contains(zone_name.as_str()) {
            continue;
        }
        info!("technitium: deleting unmanaged zone '{zone_name}'");
        if let Err(e) = delete_zone(client, &token, zone_name).await {
            error!("technitium: failed to delete zone '{zone_name}': {e}");
        }
    }

    // Create and reconcile declared zones.
    let has_viewer = config.viewer_password.as_ref().is_some_and(|p| !p.is_empty());
    for (zone_name, zone_config) in &config.zone {
        if !existing_zones.contains(zone_name.as_str()) {
            info!("technitium: creating zone '{zone_name}'");
            if let Err(e) = create_zone(client, &token, zone_name).await {
                error!("technitium: failed to create zone '{zone_name}': {e}");
                continue;
            }
        }

        reconcile_zone(client, &token, zone_name, zone_config).await;

        // Grant viewer read access to the zone.
        if has_viewer {
            if let Err(e) = grant_zone_view(client, &token, zone_name).await {
                error!("technitium: failed to set zone permissions for '{zone_name}': {e}");
            }
        }
    }

    true
}
