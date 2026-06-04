use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::routing::get;
use futures_util::stream::{Stream, StreamExt};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_error, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route("/", get_with_docs!(get_services_config))
        .route("/events", get(sse_handler))
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

/// SSE endpoint for services-config change notifications.
///
/// Emits a `config-changed` event whenever the HCL config file is modified.
/// Protected by mTLS policy at /internal/*.
pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    tracing::info!("SSE client connected to /internal/services-config/events");
    let config_rx = state.config_changed_tx.subscribe();

    let stream = BroadcastStream::new(config_rx).filter_map(|result| async move {
        match result {
            Ok(()) => Some(Ok(Event::default().event("config-changed").data("reload"))),
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text(""),
    )
}
