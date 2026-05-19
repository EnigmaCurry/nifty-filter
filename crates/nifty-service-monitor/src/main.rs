mod config;
mod technitium;
mod tofu;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use log::{debug, info, warn};

use config::{ApiResponse, ServicesConfig};

#[derive(Parser)]
#[command(name = "nifty-service-monitor")]
struct Cli {
    /// Router API base URL (e.g. https://10.99.2.1:3000)
    #[arg(long, env = "MONITOR_ROUTER_URL")]
    router_url: String,

    /// Poll interval in seconds
    #[arg(long, env = "MONITOR_POLL_INTERVAL", default_value = "15")]
    poll_interval: u64,

    /// Directory for persistent state (certificate pins, etc.)
    #[arg(long, env = "MONITOR_STATE_DIR", default_value = "/var/lib/nifty-service-monitor")]
    state_dir: PathBuf,
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

/// Per-service state tracked across polling iterations.
#[derive(Default)]
struct ServiceState {
    technitium: technitium::TechnitiumState,
}

async fn poll_and_apply(
    client: &reqwest::Client,
    config: &ServicesConfig,
    state: &mut ServiceState,
) {
    if let Some(ref tech) = config.technitium {
        technitium::apply(client, tech, &mut state.technitium).await;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let interval = Duration::from_secs(cli.poll_interval);
    let client = build_client(&cli.state_dir);

    info!(
        "starting nifty-service-monitor (router={}, interval={}s)",
        cli.router_url, cli.poll_interval
    );

    let mut state = ServiceState::default();
    let mut config_fetched = false;

    loop {
        match fetch_config(&client, &cli.router_url).await {
            Ok(config) => {
                if !config_fetched {
                    info!("successfully fetched services config from router");
                    config_fetched = true;
                }
                debug!("polling services config");
                poll_and_apply(&client, &config, &mut state).await;
            }
            Err(e) => {
                warn!("failed to fetch services config: {e}");
                config_fetched = false;
            }
        }

        tokio::time::sleep(interval).await;
    }
}
