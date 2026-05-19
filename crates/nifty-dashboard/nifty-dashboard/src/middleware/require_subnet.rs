use axum::{
    extract::ConnectInfo,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

/// Middleware that restricts access to clients within a specific subnet.
pub async fn require_subnet(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let subnet = services_subnet();
    if !subnet.contains(peer.ip()) {
        tracing::warn!(
            "require_subnet: rejecting peer {} (not in {})",
            peer.ip(),
            subnet
        );
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

/// Read the allowed subnet from NIFTY_SERVICES_SUBNET env var,
/// defaulting to 10.99.2.0/24.
fn services_subnet() -> ipnetwork::IpNetwork {
    std::env::var("NIFTY_SERVICES_SUBNET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "10.99.2.0/24".parse().unwrap())
}

