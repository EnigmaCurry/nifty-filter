use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::{
    Json,
    extract::State,
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    errors::ErrorBody,
    middleware::auth::AuthenticationMethod,
    prelude::*,
    response::{ApiJson, ApiResponse, json_ok},
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(config_json))
}

#[derive(Debug, Serialize, JsonSchema)]
struct ConfigData {
    #[serde(default)]
    pub auth_method: AuthenticationMethod,
}

/// The shape of the JSON we send back from Config API.
#[derive(Debug, Serialize, JsonSchema)]
struct ConfigResponse {
    config: ConfigData,
}

#[api_doc(
    id = "config",
    tag = "system",
    ok = "Json<ApiResponse<ConfigResponse>>",
    err = "Json<ErrorBody>"
)]
/// Get server config data
async fn config_json(State(state): State<AppState>) -> ApiJson<ConfigResponse> {
    json_ok(ConfigResponse {
        config: ConfigData {
            auth_method: state.auth_config.method,
        },
    })
}
