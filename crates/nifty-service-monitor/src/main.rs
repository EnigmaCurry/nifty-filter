mod config;
mod ddns;
mod technitium;
mod traefik;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use tokio::sync::mpsc;

use config::{ApiResponse, ServicesConfig};

/// Maximum consecutive failures during startup (before first success).
/// With a 15s poll interval this is ~5 minutes — generous for cold boot.
const MAX_STARTUP_FAILURES: u32 = 20;

/// Maximum consecutive failures after the service has been healthy.
/// With a 15s poll interval this is ~45 seconds.
const MAX_RUNTIME_FAILURES: u32 = 3;

#[derive(Parser)]
#[command(name = "nifty-service-monitor")]
struct Cli {
    /// Router API base URL (e.g. https://10.99.2.1:3000)
    #[arg(long, env = "MONITOR_ROUTER_URL")]
    router_url: String,

    /// Poll interval in seconds (fallback when SSE is active)
    #[arg(long, env = "MONITOR_POLL_INTERVAL", default_value = "60")]
    poll_interval: u64,

    /// Directory for persistent state (admin passwords, etc.)
    #[arg(long, env = "MONITOR_STATE_DIR", default_value = "/var/lib/nifty-service-monitor")]
    state_dir: PathBuf,

    /// Path to client certificate PEM for mTLS
    #[arg(long, env = "MONITOR_CLIENT_CERT")]
    client_cert: Option<PathBuf>,

    /// Path to client key PEM for mTLS
    #[arg(long, env = "MONITOR_CLIENT_KEY")]
    client_key: Option<PathBuf>,

    /// Traefik dynamic config directory (for writing service router configs)
    #[arg(long, env = "MONITOR_TRAEFIK_DYNAMIC_DIR")]
    traefik_dynamic_dir: Option<PathBuf>,

    /// Path to write the ddns-updater config.json
    #[arg(long, env = "MONITOR_DDNS_CONFIG_PATH")]
    ddns_config_path: Option<PathBuf>,
}

fn build_client(
    client_cert: Option<&std::path::Path>,
    client_key: Option<&std::path::Path>,
) -> reqwest::Client {
    // Load system root certificates (includes Step-CA root via security.pki.certificateFiles)
    let mut root_store = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().expect("failed to load system root certs") {
        root_store.add(cert).expect("failed to add system root cert");
    }

    let tls_config = if let (Some(cert_path), Some(key_path)) = (client_cert, client_key) {
        // mTLS: present client certificate
        let certs = load_pem_certs(cert_path);
        let key = load_pem_key(key_path);
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, key)
            .expect("failed to configure client auth cert")
    } else {
        // No client cert — still verify server via system roots
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

fn load_pem_certs(path: &std::path::Path) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    let data = std::fs::read(path)
        .unwrap_or_else(|e| panic!("failed to read cert file '{}': {e}", path.display()));
    rustls_pemfile::certs(&mut &data[..])
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| panic!("failed to parse PEM certs from '{}': {e}", path.display()))
}

fn load_pem_key(path: &std::path::Path) -> rustls::pki_types::PrivateKeyDer<'static> {
    let data = std::fs::read(path)
        .unwrap_or_else(|e| panic!("failed to read key file '{}': {e}", path.display()));
    rustls_pemfile::private_key(&mut &data[..])
        .unwrap_or_else(|e| panic!("failed to parse PEM key from '{}': {e}", path.display()))
        .unwrap_or_else(|| panic!("no private key found in '{}'", path.display()))
}

/// Format a reqwest error with its full source chain for debugging.
fn format_error(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(cause) = source {
        msg.push_str(&format!(": {cause}"));
        source = std::error::Error::source(cause);
    }
    msg
}

async fn fetch_config(client: &reqwest::Client, router_url: &str) -> Result<ServicesConfig, String> {
    let url = format!("{router_url}/internal/services-config");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("request failed: {}", format_error(&e)))?;

    let api: ApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    if let Some(err) = api.error {
        return Err(format!("API returned error: {err}"));
    }

    api.data
        .map(|d| d.services)
        .ok_or_else(|| "response missing data field".to_string())
}

/// Maintain a persistent SSE connection to the dashboard, sending a
/// notification on the channel whenever a `config-changed` event arrives.
/// Automatically reconnects on error with a 5-second backoff.
async fn sse_listener(client: reqwest::Client, url: String, tx: mpsc::Sender<()>) {
    loop {
        debug!("connecting to SSE endpoint: {url}");
        match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    warn!("SSE endpoint returned status {}", resp.status());
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                info!("SSE connection established");
                let mut stream = resp.bytes_stream();
                let mut buf = String::new();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(pos) = buf.find("\n\n") {
                                let block = &buf[..pos];
                                if block.contains("event: config-changed") {
                                    info!("received config-changed event via SSE");
                                    let _ = tx.try_send(());
                                }
                                buf = buf[pos + 2..].to_string();
                            }
                            // Prevent unbounded growth from partial data
                            if buf.len() > 4096 {
                                buf.clear();
                            }
                        }
                        Err(e) => {
                            warn!("SSE stream error: {e}");
                            break;
                        }
                    }
                }
                warn!("SSE connection closed, reconnecting");
            }
            Err(e) => {
                warn!("SSE connection failed: {}", format_error(&e));
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Per-service state tracked across polling iterations.
struct ServiceState {
    technitium: technitium::TechnitiumState,
}

/// Run one poll cycle. Returns true if all services applied successfully.
async fn poll_and_apply(
    client: &reqwest::Client,
    config: &ServicesConfig,
    state: &mut ServiceState,
    traefik_dynamic_dir: Option<&Path>,
    ddns_config_path: Option<&Path>,
) -> bool {
    let mut ok = true;

    if let Some(ref dns) = config.dns {
        if !technitium::apply(client, dns, &config.host.domain, &mut state.technitium).await {
            ok = false;
        }
    }

    // Write Traefik dynamic configs for all declared routes.
    if let Some(dir) = traefik_dynamic_dir {
        traefik::write_routes(dir, &config.host.domain, config.traefik.as_ref());
    }

    // Write ddns-updater config.json (systemd path unit restarts the container).
    if let Some(path) = ddns_config_path {
        if let Some(ref ddns) = config.ddns {
            if !ddns::write_config(path, ddns) {
                ok = false;
            }
        }
    }

    ok
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stdout)
        .init();

    let cli = Cli::parse();
    let interval = Duration::from_secs(cli.poll_interval);
    let client = build_client(cli.client_cert.as_deref(), cli.client_key.as_deref());

    info!(
        "starting nifty-service-monitor (router={}, interval={}s)",
        cli.router_url, cli.poll_interval
    );

    // Spawn SSE listener for real-time config change notifications
    let (sse_tx, mut sse_rx) = mpsc::channel::<()>(1);
    let sse_url = format!("{}/internal/services-config/events", cli.router_url);
    tokio::spawn(sse_listener(client.clone(), sse_url, sse_tx));

    let mut state = ServiceState {
        technitium: technitium::TechnitiumState::new(&cli.state_dir),
    };
    let mut config_fetched = false;
    let mut ever_healthy = false;
    let mut consecutive_failures: u32 = 0;

    loop {
        let cycle_ok = match fetch_config(&client, &cli.router_url).await {
            Ok(config) => {
                if !config_fetched {
                    info!("successfully fetched services config from router");
                    config_fetched = true;
                }
                debug!("applying services config");
                poll_and_apply(&client, &config, &mut state, cli.traefik_dynamic_dir.as_deref(), cli.ddns_config_path.as_deref()).await
            }
            Err(e) => {
                warn!("failed to fetch services config: {e}");
                config_fetched = false;
                false
            }
        };

        if cycle_ok {
            consecutive_failures = 0;
            ever_healthy = true;
        } else {
            consecutive_failures += 1;
            let limit = if ever_healthy { MAX_RUNTIME_FAILURES } else { MAX_STARTUP_FAILURES };
            if consecutive_failures >= limit {
                error!(
                    "exiting after {} consecutive failures (systemd will restart)",
                    consecutive_failures
                );
                return ExitCode::FAILURE;
            }
        }

        // Wait for SSE config-changed event or fall back to periodic poll
        tokio::select! {
            _ = sse_rx.recv() => {
                debug!("triggered by SSE config-changed event");
            }
            _ = tokio::time::sleep(interval) => {
                debug!("periodic poll (no SSE events in {}s)", cli.poll_interval);
            }
        }
    }
}
