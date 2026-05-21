use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ApiResponse {
    pub error: Option<String>,
    pub data: Option<ApiData>,
}

#[derive(Deserialize)]
pub struct ApiData {
    pub services: ServicesConfig,
}

#[derive(Deserialize, Default)]
pub struct ServicesConfig {
    #[serde(default)]
    pub host: HostConfig,
    pub dns: Option<DnsServiceConfig>,
    #[serde(default)]
    pub traefik: Option<TraefikConfig>,
    pub ddns: Option<DdnsConfig>,
}

#[derive(Deserialize, Default)]
pub struct TraefikConfig {
    #[serde(default)]
    pub route: HashMap<String, RouteConfig>,
}

#[derive(Deserialize)]
pub struct RouteConfig {
    pub backend: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

#[derive(Deserialize)]
pub struct HostConfig {
    #[allow(dead_code)]
    pub ip_address: Option<String>,
    #[serde(default = "default_domain")]
    pub domain: String,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            ip_address: None,
            domain: default_domain(),
        }
    }
}

fn default_domain() -> String {
    "nifty.internal".to_string()
}

#[derive(Deserialize)]
pub struct DnsServiceConfig {
    pub viewer_password: Option<String>,
    #[serde(default)]
    pub zone: HashMap<String, ZoneConfig>,
    #[serde(default)]
    pub unmanaged_zones: Vec<String>,
    #[serde(default)]
    pub forwarders: Vec<String>,
    pub forwarder_protocol: Option<String>,
    pub forwarder_concurrency: Option<u8>,
}

/// Zone configuration using type-grouped records.
///
/// Each record type (A, AAAA, CNAME, etc.) is a map of hostname -> value.
/// Use "@" for the zone apex, dotted names like "app.dev" for subdomains.
///
/// Simple types (A, AAAA, CNAME, NS, TXT) map hostname to a string value.
/// Complex types (MX, SRV, CAA) map hostname to a struct with fields.
#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct ZoneConfig {
    #[serde(default)]
    pub A: HashMap<String, String>,
    #[serde(default)]
    pub AAAA: HashMap<String, String>,
    #[serde(default)]
    pub CNAME: HashMap<String, String>,
    #[serde(default)]
    pub NS: HashMap<String, String>,
    #[serde(default)]
    pub TXT: HashMap<String, String>,
    #[serde(default)]
    pub MX: HashMap<String, MxRecord>,
    #[serde(default)]
    pub SRV: HashMap<String, SrvRecord>,
    #[serde(default)]
    pub CAA: HashMap<String, CaaRecord>,
}

#[derive(Deserialize)]
pub struct MxRecord {
    pub exchange: String,
    #[serde(default = "default_mx_preference")]
    pub preference: u16,
    pub ttl: Option<u32>,
}

fn default_mx_preference() -> u16 {
    10
}

#[derive(Deserialize)]
pub struct SrvRecord {
    pub target: String,
    pub port: u16,
    #[serde(default)]
    pub priority: u16,
    #[serde(default)]
    pub weight: u16,
    pub ttl: Option<u32>,
}

#[derive(Deserialize)]
pub struct CaaRecord {
    pub tag: String,
    pub value: String,
    #[serde(default)]
    pub flags: u8,
    pub ttl: Option<u32>,
}

#[derive(Deserialize)]
pub struct DdnsConfig {
    /// Period is consumed by the Nix container module as an env var,
    /// not by the service-monitor, but we parse it to avoid unknown-field errors.
    #[allow(dead_code)]
    #[serde(default = "default_ddns_period")]
    pub period: String,
    #[serde(default)]
    pub record: HashMap<String, DdnsRecord>,
}

fn default_ddns_period() -> String {
    "5m".to_string()
}

/// A single DDNS record entry. The `provider` field is required; all other
/// fields are provider-specific and passed through to ddns-updater's config.json.
#[derive(Deserialize)]
pub struct DdnsRecord {
    pub provider: String,
    /// All remaining provider-specific fields (token, zone_identifier, etc.)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// The JSON format that ddns-updater expects in its config.json.
#[derive(Serialize)]
pub struct DdnsUpdaterConfig {
    pub settings: Vec<DdnsUpdaterEntry>,
}

#[derive(Serialize)]
pub struct DdnsUpdaterEntry {
    pub provider: String,
    pub domain: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

