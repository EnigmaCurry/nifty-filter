use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_error, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_services_config))
}

#[derive(Serialize, JsonSchema)]
struct ServicesConfigResponse {
    services: Value,
}

#[api_doc(
    id = "get_services_config",
    tag = "services-config",
    ok = "Json<ApiResponse<ServicesConfigResponse>>",
    err = "Json<ErrorBody>"
)]
/// Services configuration
///
/// Returns the "services" section of the HCL configuration as JSON.
/// Access is restricted to clients in the configured services subnet.
async fn get_services_config(_state: State<AppState>) -> ApiJson<ServicesConfigResponse> {
    let path = crate::config_watcher::config_file_path();
    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot read config file: {e}"),
            );
        }
    };

    let config = match crate::routes::status::parse_hcl_to_json(&contents) {
        Ok(v) => v,
        Err(e) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
    };

    match config.get("services") {
        Some(services) => json_ok(ServicesConfigResponse {
            services: services.clone(),
        }),
        None => json_error(StatusCode::NOT_FOUND, "no services block in config"),
    }
}
