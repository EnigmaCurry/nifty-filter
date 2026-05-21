use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::Serialize;
use tokio::process::Command;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_mdns))
}

#[derive(Serialize, JsonSchema)]
struct MdnsResponse {
    /// Interfaces participating in mDNS reflection
    interfaces: Vec<String>,
    /// Discovered mDNS services being reflected
    services: Vec<MdnsService>,
    /// Whether the avahi service is active
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Clone, Serialize, JsonSchema)]
struct MdnsService {
    interface: String,
    protocol: String,
    name: String,
    service_type: String,
    domain: String,
    hostname: String,
    address: String,
    port: u16,
}

#[api_doc(
    id = "get_mdns",
    tag = "mdns",
    ok = "Json<ApiResponse<MdnsResponse>>",
    err = "Json<ErrorBody>"
)]
/// mDNS reflector status
///
/// Returns the avahi mDNS reflector configuration (which interfaces are
/// reflecting) and currently discovered services from avahi-browse.
async fn get_mdns(_state: State<AppState>) -> ApiJson<MdnsResponse> {
    let (interfaces, active) = read_avahi_config().await;

    if interfaces.is_empty() {
        return json_ok(MdnsResponse {
            interfaces: vec![],
            services: vec![],
            active: false,
            error: None,
        });
    }

    let (services, browse_error) = browse_services().await;

    json_ok(MdnsResponse {
        interfaces,
        services,
        active,
        error: browse_error,
    })
}

/// Read the generated avahi config to extract reflecting interfaces,
/// and check systemd for service status.
async fn read_avahi_config() -> (Vec<String>, bool) {
    let (config_result, status_result) = tokio::join!(
        tokio::fs::read_to_string("/run/avahi-daemon/avahi-daemon.conf"),
        Command::new("systemctl")
            .args(["is-active", "--quiet", "nifty-avahi"])
            .status()
    );

    let active = status_result.map(|s| s.success()).unwrap_or(false);

    let interfaces = match config_result {
        Ok(contents) => {
            contents
                .lines()
                .find_map(|line| line.strip_prefix("allow-interfaces="))
                .map(|val| val.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default()
        }
        Err(_) => vec![],
    };

    (interfaces, active)
}

/// Cached result of the last avahi-browse run, shared across requests.
static MDNS_CACHE: std::sync::LazyLock<tokio::sync::Mutex<MdnsCache>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(MdnsCache::default()));

#[derive(Default)]
struct MdnsCache {
    services: Vec<MdnsService>,
    error: Option<String>,
    last_refresh: Option<std::time::Instant>,
    refresh_in_progress: bool,
}

/// Return cached mDNS services. Triggers a background refresh if the cache
/// is older than 10 seconds. The browse itself takes ~3 seconds (we let
/// avahi-browse listen for multicast responses), so we never block the API.
async fn browse_services() -> (Vec<MdnsService>, Option<String>) {
    let mut cache = MDNS_CACHE.lock().await;

    let stale = cache
        .last_refresh
        .map(|t| t.elapsed() > std::time::Duration::from_secs(10))
        .unwrap_or(true);

    if stale && !cache.refresh_in_progress {
        cache.refresh_in_progress = true;
        tokio::spawn(async {
            let (services, error) = run_avahi_browse().await;
            let mut cache = MDNS_CACHE.lock().await;
            cache.services = services;
            cache.error = error;
            cache.last_refresh = Some(std::time::Instant::now());
            cache.refresh_in_progress = false;
        });
    }

    (cache.services.clone(), cache.error.clone())
}

/// Run avahi-browse for 3 seconds to collect mDNS service announcements.
/// The reflector daemon doesn't maintain a browse cache, so we need to
/// actively listen for multicast responses rather than using -t (terminate).
async fn run_avahi_browse() -> (Vec<MdnsService>, Option<String>) {
    let mut child = match Command::new("avahi-browse")
        .args(["-a", "-p", "-r"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (vec![], Some(format!("failed to run avahi-browse: {e}"))),
    };

    // Let it listen for 3 seconds, then kill it
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let _ = child.kill().await;

    let output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return (vec![], Some(format!("avahi-browse wait failed: {e}"))),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let services = parse_avahi_browse(&stdout);
    let error = if services.is_empty() && !stderr.is_empty() {
        Some(format!("avahi-browse: {}", stderr.trim()))
    } else {
        None
    };

    (services, error)
}

/// Decode avahi's parseable-mode escaping (e.g. `\032` → space).
/// Avahi uses decimal byte values (not octal) in `\NNN` sequences.
fn decode_avahi_escaped(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut digits = String::new();
            for _ in 0..3 {
                let mut peek = chars.clone();
                if let Some(d) = peek.next() {
                    if d.is_ascii_digit() {
                        digits.push(d);
                        chars = peek;
                    } else {
                        break;
                    }
                }
            }
            if digits.len() == 3 {
                if let Ok(byte) = digits.parse::<u8>() {
                    result.push(byte as char);
                } else {
                    result.push('\\');
                    result.push_str(&digits);
                }
            } else {
                result.push('\\');
                result.push_str(&digits);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse avahi-browse resolved output.
/// Format: =;interface;protocol;name;type;domain;hostname;address;port;txt
fn parse_avahi_browse(output: &str) -> Vec<MdnsService> {
    output
        .lines()
        .filter(|line| line.starts_with('='))
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(10, ';').collect();
            if parts.len() < 9 {
                return None;
            }
            Some(MdnsService {
                interface: parts[1].to_string(),
                protocol: match parts[2] {
                    "0" => "IPv4".to_string(),
                    "1" => "IPv6".to_string(),
                    other => other.to_string(),
                },
                name: decode_avahi_escaped(parts[3]),
                service_type: parts[4].to_string(),
                domain: parts[5].to_string(),
                hostname: parts[6].to_string(),
                address: parts[7].to_string(),
                port: parts[8].parse().unwrap_or(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_avahi_browse_resolved() {
        let output = "\
+;trusted;0;Living Room Speaker;_googlecast._tcp;local
=;trusted;0;Living Room Speaker;_googlecast._tcp;local;speaker.local;192.168.10.50;8009;
=;iot;0;Bedroom Light;_hap._tcp;local;light.local;192.168.20.10;80;
+;trusted;0;My NAS;_smb._tcp;local
=;trusted;0;My NAS;_smb._tcp;local;nas.local;192.168.10.100;445;
";
        let services = parse_avahi_browse(output);
        // Only '=' lines are parsed
        assert_eq!(services.len(), 3);

        assert_eq!(services[0].interface, "trusted");
        assert_eq!(services[0].protocol, "IPv4");
        assert_eq!(services[0].name, "Living Room Speaker");
        assert_eq!(services[0].service_type, "_googlecast._tcp");
        assert_eq!(services[0].hostname, "speaker.local");
        assert_eq!(services[0].address, "192.168.10.50");
        assert_eq!(services[0].port, 8009);

        assert_eq!(services[1].interface, "iot");
        assert_eq!(services[1].name, "Bedroom Light");
        assert_eq!(services[1].address, "192.168.20.10");
        assert_eq!(services[1].port, 80);

        assert_eq!(services[2].name, "My NAS");
        assert_eq!(services[2].port, 445);
    }

    #[test]
    fn parse_avahi_browse_empty() {
        assert!(parse_avahi_browse("").is_empty());
        assert!(parse_avahi_browse("\n\n").is_empty());
    }

    #[test]
    fn parse_avahi_browse_discovery_only_ignored() {
        // '+' lines without matching '=' should not appear
        let output = "+;trusted;0;Something;_http._tcp;local\n";
        assert!(parse_avahi_browse(output).is_empty());
    }

    #[test]
    fn decode_avahi_spaces() {
        assert_eq!(decode_avahi_escaped(r"test\032from\032arch"), "test from arch");
    }

    #[test]
    fn decode_avahi_no_escapes() {
        assert_eq!(decode_avahi_escaped("Living Room Speaker"), "Living Room Speaker");
    }

    #[test]
    fn decode_avahi_special_chars() {
        // \039 = apostrophe ('), \092 = backslash (\)
        assert_eq!(decode_avahi_escaped(r"Bob\039s\032TV"), "Bob's TV");
    }

    #[test]
    fn parse_avahi_browse_escaped_names() {
        let output = "=;infra;0;test\\032from\\032arch;_http._tcp;local;arch.local;192.168.1.5;8080;\n";
        let services = parse_avahi_browse(output);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "test from arch");
        assert_eq!(services[0].address, "192.168.1.5");
        assert_eq!(services[0].port, 8080);
    }
}
