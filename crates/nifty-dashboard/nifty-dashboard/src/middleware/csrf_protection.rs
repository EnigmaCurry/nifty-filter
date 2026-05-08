use axum::{
    Json,
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::middleware::user_session::UserSession; // adjust path to where your type lives

// Methods we consider "unsafe"
fn is_state_changing(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

#[derive(Serialize)]
struct CsrfErrorBody {
    error: &'static str,
    message: &'static str,
}

fn json_error(status: StatusCode, error: &'static str, message: &'static str) -> Response {
    let body = CsrfErrorBody { error, message };
    (status, Json(body)).into_response()
}

// Simple CSRF middleware using X-CSRF-Token header and your UserSession
pub async fn csrf_middleware(user_session: UserSession, req: Request, next: Next) -> Response {
    // Skip non–state-changing methods entirely
    if !is_state_changing(req.method()) {
        return next.run(req).await;
    }

    // If for some reason your UserSession could have an empty token, treat that as "missing".
    if user_session.csrf_token.is_empty() {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "csrf_missing",
            "No CSRF token found in session",
        );
    }

    let expected = &user_session.csrf_token;

    let provided = req
        .headers()
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok());

    if provided != Some(expected.as_str()) {
        tracing::warn!("CSRF header mismatch: provided={:?}", provided);
        return json_error(
            StatusCode::UNAUTHORIZED,
            "csrf_invalid",
            "Invalid CSRF token",
        );
    }

    // All good, continue the chain
    next.run(req).await
}
