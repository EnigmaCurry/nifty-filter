use axum::extract::{FromRequestParts, Request};
use axum::http::{StatusCode, request::Parts};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use uuid::Uuid;

use crate::middleware::trusted_forwarded_for::ForwardedClientIp;
use crate::prelude::*;

const SESSION_KEY: &str = "user_session_v1";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
/// User session object contains a few global session data points per guest.
pub struct UserSession {
    /// The internal user id:
    pub user_id: i64,
    pub username: Option<String>,
    /// The raw TCP peer address as seen by our server.
    pub peer_ip: String,
    /// External user id from OAuth or other trusted source.
    pub external_user_id: Option<String>,
    #[serde(default)]
    pub is_logged_in: bool,
    pub visit_count: u64,
    pub csrf_token: String,
    /// Trusted client IP from x-forwarded-for (if enabled/valid).
    pub client_ip: Option<String>,
}

impl UserSession {
    pub async fn persist(&self, session: &Session) -> AppResult<()> {
        // tower_sessions::session::Error implements std::error::Error,
        // so `?` will turn it into AppError via your `impl<E: Error> From<E> for AppError`.
        session.insert(SESSION_KEY, self.clone()).await?;
        Ok(())
    }
}

fn generate_csrf_token() -> String {
    Uuid::new_v4().to_string()
}

/// Middleware that runs on every request and keeps the user session up to date.
///
/// Responsibilities:
/// - Ensure a CSRF token is present.
/// - Increment visit_count.
/// - Copy the trusted client IP (if available) into the session.
/// - Record peer_ip as seen by our server.
pub async fn user_session_middleware(
    session: Session,
    mut req: Request,
    next: Next,
) -> AppResult<Response> {
    // Load existing typed session or start from default.
    let mut data: UserSession = session.get(SESSION_KEY).await?.unwrap_or_default();
    debug!("Loaded UserSession: {data:?}");
    if data.csrf_token.is_empty() {
        data.csrf_token = generate_csrf_token();
    }

    data.visit_count = data.visit_count.saturating_add(1);

    if let Some(fwd) = req.extensions().get::<ForwardedClientIp>() {
        data.peer_ip = fwd.peer_ip.to_string();
        data.client_ip = fwd.client_ip.map(|ip| ip.to_string());
    }
    data.persist(&session).await?;

    req.extensions_mut().insert(data);

    Ok(next.run(req).await)
}

// --- Extractor for handlers --------------------------------------------------

impl<S> FromRequestParts<S> for UserSession
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Prefer the value computed by the middleware, if present.
        if let Some(existing) = parts.extensions.get::<UserSession>() {
            return Ok(existing.clone());
        }

        // Fallback: load directly from the session (e.g. if a route isn’t
        // behind the middleware for some reason).
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session error"))?;

        let data: UserSession = session
            .get(SESSION_KEY)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session load error"))?
            .unwrap_or_default();

        Ok(data)
    }
}
