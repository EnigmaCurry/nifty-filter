use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use crate::tls::mtls::PeerCertMap;

/// A single mTLS authorization policy.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct MtlsPolicy {
    pub name: String,
    /// Allowed CN patterns. Empty = public (no cert required).
    #[serde(default)]
    pub cn: Vec<String>,
    /// URL path patterns this policy matches.
    pub paths: Vec<String>,
}

/// Configuration for the mTLS middleware.
#[derive(Clone)]
pub struct MtlsConfig {
    pub peer_certs: PeerCertMap,
    /// Ordered policies — first match wins. No match = deny.
    pub policies: Vec<MtlsPolicy>,
}

/// Middleware that enforces mTLS authorization via ordered policies.
///
/// For each request, policies are evaluated in order. The first policy whose
/// path pattern matches the request URI wins. If that policy has a non-empty
/// `cn` list, the client must present a certificate with a matching CN.
/// If no policy matches, the request is denied (403).
pub async fn require_mtls(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(mtls): State<MtlsConfig>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Find the first matching policy
    let matched = mtls.policies.iter().find(|p| {
        p.paths.iter().any(|pattern| path_matches(path, pattern))
    });

    let Some(policy) = matched else {
        tracing::warn!("mTLS: no policy matches path '{path}' from {peer}");
        return (StatusCode::FORBIDDEN, "no mTLS policy matches this path\n").into_response();
    };

    // Empty cn list = public access
    if policy.cn.is_empty() {
        tracing::debug!("mTLS: policy '{}' allows public access to '{path}'", policy.name);
        return next.run(req).await;
    }

    // Policy requires a client cert — look up peer certs from the shared map
    let certs = mtls.peer_certs.get(&peer).map(|entry| entry.value().clone());

    let Some(certs) = certs else {
        tracing::warn!("mTLS: policy '{}' requires cert for '{path}' but no entry for {peer}", policy.name);
        return (StatusCode::FORBIDDEN, "client certificate required\n").into_response();
    };

    if certs.is_empty() {
        tracing::warn!("mTLS: policy '{}' requires cert for '{path}' but {peer} presented none", policy.name);
        return (StatusCode::FORBIDDEN, "client certificate required\n").into_response();
    }

    // Parse the leaf certificate's CN and check against the policy's whitelist
    match extract_cn(&certs[0]) {
        Ok(cn) => {
            if !cn_matches_list(&cn, &policy.cn) {
                tracing::warn!(
                    "mTLS: policy '{}' rejects CN '{cn}' from {peer} for '{path}'",
                    policy.name
                );
                return (StatusCode::FORBIDDEN, "certificate CN not authorized\n").into_response();
            }
            tracing::debug!(
                "mTLS: policy '{}' accepted CN '{cn}' from {peer} for '{path}'",
                policy.name
            );
        }
        Err(e) => {
            tracing::warn!("mTLS: failed to parse cert CN from {peer}: {e}");
            return (StatusCode::FORBIDDEN, "invalid client certificate\n").into_response();
        }
    }

    next.run(req).await
}

fn extract_cn(cert_der: &[u8]) -> anyhow::Result<String> {
    use x509_parser::prelude::*;

    let (_, cert) =
        parse_x509_certificate(cert_der).map_err(|e| anyhow::anyhow!("x509 parse: {e}"))?;

    for rdn in cert.subject().iter() {
        for attr in rdn.iter() {
            if attr.attr_type() == &oid_registry::OID_X509_COMMON_NAME {
                return Ok(attr.as_str()?.to_string());
            }
        }
    }

    anyhow::bail!("no CN found in certificate subject")
}

fn cn_matches_list(cn: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_match(cn, pattern))
}

/// Match a path against a pattern. Supports:
/// - Exact match: `/healthz` matches `/healthz`
/// - Glob suffix: `/internal/*` matches `/internal/foo` and `/internal/foo/bar`
/// - Wildcard: `/*` matches everything
fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern == "/*" || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    path == pattern
}

/// Simple glob matching for CN patterns:
/// - `*` matches everything
/// - `*.suffix` matches any name ending in `.suffix`
/// - Otherwise exact match
fn glob_match(s: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return s.ends_with(&format!(".{suffix}"));
    }
    s == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_exact() {
        assert!(path_matches("/healthz", "/healthz"));
        assert!(!path_matches("/healthz/foo", "/healthz"));
        assert!(!path_matches("/other", "/healthz"));
    }

    #[test]
    fn path_glob_suffix() {
        assert!(path_matches("/internal/foo", "/internal/*"));
        assert!(path_matches("/internal/foo/bar", "/internal/*"));
        assert!(path_matches("/internal/", "/internal/*"));
        // The prefix itself without trailing slash should also match
        assert!(path_matches("/internal", "/internal/*"));
        assert!(!path_matches("/internalx", "/internal/*"));
    }

    #[test]
    fn path_wildcard() {
        assert!(path_matches("/anything", "/*"));
        assert!(path_matches("/", "/*"));
        assert!(path_matches("/a/b/c", "/*"));
        assert!(path_matches("/anything", "*"));
    }

    #[test]
    fn cn_exact() {
        assert!(glob_match("dashboard.nifty.internal", "dashboard.nifty.internal"));
        assert!(!glob_match("other.nifty.internal", "dashboard.nifty.internal"));
    }

    #[test]
    fn cn_wildcard() {
        assert!(glob_match("dashboard.nifty.internal", "*.nifty.internal"));
        assert!(glob_match("traefik.nifty.internal", "*.nifty.internal"));
        assert!(!glob_match("nifty.internal", "*.nifty.internal"));
    }

    #[test]
    fn cn_star() {
        assert!(glob_match("anything", "*"));
    }

    #[test]
    fn policy_order_first_match_wins() {
        let policies = vec![
            MtlsPolicy {
                name: "apps".into(),
                cn: vec!["service-monitor.nifty.internal".into()],
                paths: vec!["/internal/*".into()],
            },
            MtlsPolicy {
                name: "public".into(),
                cn: vec![],
                paths: vec!["/*".into()],
            },
        ];

        // /internal/foo matches "apps" first (requires cert)
        let matched = policies.iter().find(|p| {
            p.paths.iter().any(|pat| path_matches("/internal/foo", pat))
        });
        assert_eq!(matched.unwrap().name, "apps");

        // /api/hello matches "public" (no cert needed)
        let matched = policies.iter().find(|p| {
            p.paths.iter().any(|pat| path_matches("/api/hello", pat))
        });
        assert_eq!(matched.unwrap().name, "public");
    }
}
