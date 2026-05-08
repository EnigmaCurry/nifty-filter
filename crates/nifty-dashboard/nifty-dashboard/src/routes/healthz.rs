use aide::axum::ApiRouter;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;

use api_doc_macros::{api_doc, get_with_docs};

use crate::errors::ErrorBody;
use crate::response::{ApiJson, ApiResponse, json_error, json_ok};
use crate::server::AppState;

#[derive(Serialize, JsonSchema)]
pub struct HealthPayload {
    pub status: &'static str,
}

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(healthz))
}

#[api_doc(
    tag = "system",
    ok = "Json<ApiResponse<HealthPayload>>",
    err = "Json<ErrorBody>"
)]
/// Get API status
///
/// Returns 200 if the server is healthy, or 500 if there is an error.
async fn healthz(state: State<AppState>) -> ApiJson<HealthPayload> {
    if let Err(e) = sqlx::query("SELECT 1").execute(&state.db).await {
        tracing::error!("healthz DB check failed: {e}");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "database unavailable");
    }
    json_ok(HealthPayload { status: "healthy" })
}
