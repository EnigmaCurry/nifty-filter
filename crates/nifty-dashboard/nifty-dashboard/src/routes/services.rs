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
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_services))
}

#[derive(Serialize, JsonSchema)]
struct ServicesResponse {
    nifty: Vec<ServiceInfo>,
    failed: Vec<ServiceInfo>,
}

#[derive(Serialize, JsonSchema)]
struct ServiceInfo {
    name: String,
    active_state: String,
    sub_state: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
}

#[api_doc(
    id = "get_services",
    tag = "services",
    ok = "Json<ApiResponse<ServicesResponse>>",
    err = "Json<ErrorBody>"
)]
/// Systemd services status
///
/// Returns status of nifty-* services and any other failed services.
async fn get_services(_state: State<AppState>) -> ApiJson<ServicesResponse> {
    let (nifty, failed) = tokio::join!(list_nifty_services(), list_failed_services());
    json_ok(ServicesResponse { nifty, failed })
}

async fn list_nifty_services() -> Vec<ServiceInfo> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--no-legend",
            "--plain",
            "nifty-*",
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    parse_systemctl_output(&String::from_utf8_lossy(&output.stdout))
}

async fn list_failed_services() -> Vec<ServiceInfo> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--state=failed",
            "--no-pager",
            "--no-legend",
            "--plain",
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let all_failed = parse_systemctl_output(&String::from_utf8_lossy(&output.stdout));
    // Exclude nifty-* services (already shown in the nifty section)
    all_failed
        .into_iter()
        .filter(|s| !s.name.starts_with("nifty-"))
        .collect()
}

fn parse_systemctl_output(stdout: &str) -> Vec<ServiceInfo> {
    // Each line: UNIT LOAD ACTIVE SUB DESCRIPTION...
    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                return None;
            }
            let name = parts[0]
                .strip_suffix(".service")
                .unwrap_or(parts[0])
                .to_string();
            let active_state = parts[2].to_string();
            let sub_state = parts[3].to_string();
            let description = parts[4..].join(" ");
            Some(ServiceInfo {
                name,
                active_state,
                sub_state,
                description,
                since: None,
            })
        })
        .collect()
}
