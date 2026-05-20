mod config;
mod technitium;
mod tofu;
mod traefik;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
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

    /// Directory for persistent state (certificate pins, etc.)
    #[arg(long, env = "MONITOR_STATE_DIR", default_value = "/var/lib/nifty-service-monitor")]
    state_dir: PathBuf,

    /// Traefik dynamic config directory (for writing service router configs)
    #[arg(long, env = "MONITOR_TRAEFIK_DYNAMIC_DIR")]
    traefik_dynamic_dir: Option<PathBuf>,
}

fn build_client(state_dir: &std::path::Path) -> reqwest::Client {
    let verifier = Arc::new(tofu::TofuVerifier::new(state_dir));
    let tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .build()
        .expect("failed to build HTTP client")
}

async fn fetch_config(client: &reqwest::Client, router_url: &str) -> Result<ServicesConfig, String> {
    let url = format!("{router_url}/api/services-config");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

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
                warn!("SSE connection failed: {e}");
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

    ok
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let interval = Duration::from_secs(cli.poll_interval);
    let client = build_client(&cli.state_dir);

    info!(
        "starting nifty-service-monitor (router={}, interval={}s)",
        cli.router_url, cli.poll_interval
    );

    // Spawn SSE listener for real-time config change notifications
    let (sse_tx, mut sse_rx) = mpsc::channel::<()>(1);
    let sse_url = format!("{}/api/services-config/events", cli.router_url);
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
                poll_and_apply(&client, &config, &mut state, cli.traefik_dynamic_dir.as_deref()).await
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
