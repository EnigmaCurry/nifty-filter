use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderName, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use log::warn;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

use crate::errors::CliError;

use super::auth::AuthenticationMethod;

/// Config for trusting an auth header from a forward-auth proxy (user/email).
#[derive(Clone, Debug)]
pub struct ForwardAuthConfig {
    pub method: AuthenticationMethod,
    pub trusted_header_name: HeaderName,
    pub trusted_proxy: Option<IpAddr>,
}

impl ForwardAuthConfig {
    /// Reasonable disabled default.
    #[allow(dead_code)]
    pub fn disabled() -> Self {
        Self {
            // UsernamePassword auth method disables trusted header auth.
            method: AuthenticationMethod::UsernamePassword,
            trusted_header_name: HeaderName::from_static("x-forwarded-user"),
            // None => no trusted proxy configured in disabled mode.
            trusted_proxy: None,
        }
    }

    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), CliError> {
        if matches!(self.method, AuthenticationMethod::ForwardAuth) && self.trusted_proxy.is_none()
        {
            return Err(CliError::InvalidArgs(
                "auth-trusted-proxy is required when auth-method=forward_auth".into(),
            ));
        }
        Ok(())
    }
}

/// Authenticated user email extracted from a trusted header.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ForwardAuthUser {
    pub external_id: String,
}

/// Middleware that enforces trusted-header auth for user/email.
///
/// Rules:
/// - If method = UsernamePassword: 403 if header present.
/// - If method = ForwardAuth:
///   - trusted_proxy must be Some (config validation should ensure this).
///   - Only trusted_proxy may send the header (403 otherwise).
///   - Header must be present and non-empty; first comma-separated token is external_id.
pub async fn trusted_header_auth(
    State(cfg): State<ForwardAuthConfig>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    match cfg.method {
        AuthenticationMethod::UsernamePassword | AuthenticationMethod::Oidc => {
            // Header is not supposed to be used; treat it as suspicious.
            if req.headers().contains_key(&cfg.trusted_header_name) {
                warn!(
                    "trusted user header auth disabled, but header '{}' was present from peer {}",
                    cfg.trusted_header_name,
                    peer.ip()
                );
                return StatusCode::FORBIDDEN.into_response();
            }
            // Skip header; rely on normal login flow.
            next.run(req).await
        }

        AuthenticationMethod::ForwardAuth => {
            let Some(proxy) = cfg.trusted_proxy else {
                // Misconfiguration: ForwardAuth enabled without a trusted proxy.
                warn!(
                    "trusted_header_auth misconfigured: auth-method=forward_auth but no trusted_proxy configured"
                );
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };

            // Header sent by *any* untrusted source → reject.
            if peer.ip() != proxy && req.headers().contains_key(&cfg.trusted_header_name) {
                warn!(
                    "trusted user header auth: rejecting spoofed header '{}' from untrusted peer {} (expected {})",
                    cfg.trusted_header_name,
                    peer.ip(),
                    proxy
                );
                return StatusCode::FORBIDDEN.into_response();
            }

            // Only care about header if it comes from the trusted proxy.
            if peer.ip() == proxy {
                let raw = req
                    .headers()
                    .get(&cfg.trusted_header_name)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty());

                let first = match raw {
                    Some(v) => v.split(',').next().unwrap().trim(),
                    None => {
                        // ForwardAuth mode but no header from proxy → unauth.
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                };

                let external_id = first.to_string();
                req.extensions_mut().insert(ForwardAuthUser { external_id });
                return next.run(req).await;
            }

            // Not from proxy and no header → don't authenticate; fall through.
            next.run(req).await
        }
    }
}
