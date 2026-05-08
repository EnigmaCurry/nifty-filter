use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderName, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

/// Config for trusting a forwarded-for header from a proxy (client IP).
#[derive(Clone, Debug)]
pub struct TrustedForwardedForConfig {
    pub enabled: bool,
    pub header_name: HeaderName,
    pub trusted_proxy: Option<IpAddr>,
}

impl TrustedForwardedForConfig {
    /// Reasonable disabled default.
    #[allow(dead_code)]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            header_name: HeaderName::from_static("x-forwarded-for"),
            // None => no trusted proxy configured; feature effectively off.
            trusted_proxy: None,
        }
    }
}

/// Client IP extracted from trusted forwarded-for header *plus* the peer IP.
#[derive(Clone, Debug)]
pub struct ForwardedClientIp {
    pub peer_ip: IpAddr,
    pub client_ip: Option<IpAddr>,
}

/// Insert the `ForwardedClientIp` extension **exactly once** per request.
/// When the feature is disabled we always insert `ForwardedClientIp { client_ip: None, .. }`.
pub async fn trusted_forwarded_for(
    State(cfg): State<TrustedForwardedForConfig>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // ------------------------------------------------------------------------
    // 1️⃣  Disabled mode – punish cheaters
    // ------------------------------------------------------------------------
    if !cfg.enabled {
        // If a caller tries to cheat by sending the header, reject outright.
        if req.headers().contains_key(&cfg.header_name) {
            tracing::warn!(
                "trusted forwarded-for disabled, but header '{}' was present from peer {}",
                cfg.header_name,
                peer.ip()
            );
            return StatusCode::FORBIDDEN.into_response();
        }

        // Explicitly hide the client IP, but still record the peer IP.
        req.extensions_mut().insert(ForwardedClientIp {
            peer_ip: peer.ip(),
            client_ip: None,
        });
        return next.run(req).await;
    }

    // ------------------------------------------------------------------------
    // 2️⃣  Enabled mode – sanity-check the sender.
    // ------------------------------------------------------------------------
    match cfg.trusted_proxy {
        Some(proxy) => {
            // Header sent by *any* untrusted source → reject.
            if peer.ip() != proxy && req.headers().contains_key(&cfg.header_name) {
                tracing::warn!(
                    "trusted forwarded-for: rejecting spoofed header '{}' from untrusted peer {} (expected {})",
                    cfg.header_name,
                    peer.ip(),
                    proxy
                );
                return StatusCode::FORBIDDEN.into_response();
            }
        }
        None => {
            // Misconfig: enabled but no proxy configured.
            // Easiest safe behavior: reject if header present from anybody.
            if req.headers().contains_key(&cfg.header_name) {
                tracing::warn!(
                    "trusted forwarded-for is enabled but no trusted_proxy configured; rejecting header '{}' from peer {}",
                    cfg.header_name,
                    peer.ip()
                );
                return StatusCode::FORBIDDEN.into_response();
            }
        }
    }

    // ------------------------------------------------------------------------
    // 3️⃣  Trusted proxy – try to parse the header (if any).
    // ------------------------------------------------------------------------
    let client_ip: Option<IpAddr> = {
        // Grab the raw header value, if present.
        let raw = req
            .headers()
            .get(&cfg.header_name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        // Extract the *first* comma-separated entry and attempt to turn it into an IpAddr.
        raw.and_then(|value| {
            let first = value.split(',').next().unwrap().trim();
            IpAddr::from_str(first).ok()
        })
    };

    // --------------------------------------------------------------
    // 4️⃣  Store the result in the request extensions.
    // --------------------------------------------------------------
    match client_ip {
        Some(ip) => {
            // Valid header → we know the original client IP.
            req.extensions_mut().insert(ForwardedClientIp {
                peer_ip: peer.ip(),
                client_ip: Some(ip),
            });
        }
        None => {
            // Header absent → we explicitly *hide* the client IP.
            // Header present but unparsable → 400 Bad Request.
            if req.headers().contains_key(&cfg.header_name) {
                tracing::debug!(
                    "trusted forwarded-for: header '{}' from peer {} could not be parsed",
                    cfg.header_name,
                    peer.ip()
                );
                return StatusCode::BAD_REQUEST.into_response();
            }
            req.extensions_mut().insert(ForwardedClientIp {
                peer_ip: peer.ip(),
                client_ip: None,
            });
        }
    }

    // --------------------------------------------------------------
    // 5️⃣  Continue down the stack.
    // --------------------------------------------------------------
    next.run(req).await
}
