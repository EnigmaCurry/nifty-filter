// src/response.rs
use axum::{Json, http::StatusCode};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema)]
pub struct ApiResponse<T = ()> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

// Bundled HTTP response type
pub type ApiJson<T = ()> = (StatusCode, Json<ApiResponse<T>>);

impl ApiResponse<()> {
    /// For endpoints that don't return any data
    pub fn success() -> Self {
        Self {
            error: None,
            data: None,
        }
    }

    /// Error envelope only (no status)
    #[allow(dead_code)]
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            error: Some(msg.into()),
            data: None,
        }
    }
}

impl<T> ApiResponse<T> {
    /// Envelope with data, no status
    pub fn with_data(data: T) -> Self {
        Self {
            error: None,
            data: Some(data),
        }
    }
}

/// 200 OK with data
pub fn json_ok<T>(data: T) -> ApiJson<T> {
    (StatusCode::OK, Json(ApiResponse::with_data(data)))
}

/// 200 OK, no body (just `{}`)
pub fn json_empty_ok() -> ApiJson<()> {
    (StatusCode::OK, Json(ApiResponse::success()))
}

/// Error with custom status and no data
pub fn json_error<T>(status: StatusCode, msg: impl Into<String>) -> ApiJson<T> {
    (
        status,
        Json(ApiResponse {
            error: Some(msg.into()),
            data: None,
        }),
    )
}
