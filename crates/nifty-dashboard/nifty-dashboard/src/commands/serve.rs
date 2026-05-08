use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use crate::{
    config::{AppConfig, AuthConfig, TlsAcmeChallenge, TlsMode, database::build_db_url},
    ensure_root_dir,
    errors::CliError,
    middleware::{self, auth::AuthenticationMethod},
    server,
    util::write_files::create_private_dir_all_0700_sync,
};
use anyhow::Context;
use axum::http::HeaderName;
use tracing::{debug, error, info};

static TLS_CACHE_DIR: &str = "tls-cache";

pub struct ServePlan {
    pub addr: SocketAddr,
    pub tls_config: server::TlsConfig,
    pub db_url: String,
    pub forward_auth_cfg: middleware::trusted_header_auth::ForwardAuthConfig,
    pub forward_for_cfg: middleware::trusted_forwarded_for::TrustedForwardedForConfig,
    pub oidc_cfg: middleware::oidc::OidcConfig,
    pub session_secure: bool,
    pub session_expiry_secs: u64,
    pub session_check_secs: u64,
    pub auth_config: AuthConfig,
}

fn plan_serve(cfg: &AppConfig, root_dir: &Path) -> Result<ServePlan, CliError> {
    let addr = parse_listen_addr(cfg)?;
    let tls_config = build_tls_config(cfg, root_dir)?;
    let db_url = build_db_url(cfg.database.url.clone(), root_dir);
    let (forward_auth_cfg, forward_for_cfg, oidc_cfg) = build_auth_cfgs(cfg)?;

    Ok(ServePlan {
        addr,
        tls_config,
        db_url,
        forward_auth_cfg,
        forward_for_cfg,
        oidc_cfg,
        session_secure: true,
        session_expiry_secs: cfg.session.expiry_seconds,
        session_check_secs: cfg.session.check_seconds,
        auth_config: cfg.auth.clone(),
    })
}

pub fn serve(cfg: AppConfig, root_dir: PathBuf) -> Result<(), CliError> {
    let root_dir = ensure_root_dir(root_dir)?;

    let plan = plan_serve(&cfg, &root_dir).expect("plan");

    debug!(?cfg, "serve(): parsed cfg");
    info!("Server will listen on {}", plan.addr);
    info!("Database URL: {:?}", plan.db_url);
    debug!(
        "Session config: secure={}, expiry_secs={}, check_secs={}",
        plan.session_secure, plan.session_expiry_secs, plan.session_check_secs
    );

    let rt = create_runtime()?;
    rt.block_on(server::run(
        plan.addr,
        plan.forward_auth_cfg,
        plan.forward_for_cfg,
        plan.oidc_cfg,
        plan.db_url,
        plan.session_secure,
        plan.session_expiry_secs,
        plan.session_check_secs,
        plan.tls_config,
        plan.auth_config,
    ))
    .map_err(|e| {
        error!("server::run failed: {:#}", e);
        CliError::RuntimeError(format!("{:#}", e))
    })
}

fn parse_listen_addr(cfg: &AppConfig) -> Result<SocketAddr, CliError> {
    let ip = &cfg.network.listen_ip;
    let port = cfg.network.listen_port;
    let addr_str = format!("{ip}:{port}");

    addr_str
        .parse()
        .map_err(|e| CliError::InvalidArgs(format!("Invalid listen addr '{addr_str}': {e}")))
}

fn build_tls_config(cfg: &AppConfig, root_dir: &Path) -> Result<server::TlsConfig, CliError> {
    match cfg.tls.mode {
        TlsMode::None => {
            info!("TLS mode: none (plain HTTP).");
            Ok(server::TlsConfig::Http)
        }

        TlsMode::Manual => {
            let cert_path = cfg.tls.cert_path.clone().ok_or_else(|| {
                CliError::InvalidArgs("Missing --tls-cert for --tls-mode=manual".to_string())
            })?;
            let key_path = cfg.tls.key_path.clone().ok_or_else(|| {
                CliError::InvalidArgs("Missing --tls-key for --tls-mode=manual".to_string())
            })?;

            info!(
                "TLS mode: manual (HTTPS) – cert={}, key={}",
                cert_path.display(),
                key_path.display()
            );

            Ok(server::TlsConfig::RustlsFiles {
                cert_path,
                key_path,
            })
        }

        TlsMode::SelfSigned => {
            let cache_dir = if cfg.tls.self_signed_ephemeral {
                None
            } else {
                Some(root_dir.join(TLS_CACHE_DIR))
            };
            let sans = cfg.tls.sans.0.clone();
            let leaf_valid_secs = cfg.tls.effective_self_signed_valid_seconds();
            let ca_valid_secs = cfg.tls.effective_ca_cert_valid_seconds();

            info!(
                "TLS mode: self-signed (HTTPS) – cache_dir={:?}, sans={:?}",
                cache_dir, sans
            );
            debug!(
                "TLS leaf_valid_secs={}, ca_valid_secs={}",
                leaf_valid_secs, ca_valid_secs
            );

            Ok(server::TlsConfig::SelfSigned {
                cache_dir,
                sans,
                leaf_valid_secs,
                ca_valid_secs,
            })
        }

        TlsMode::Acme => {
            let cache_dir: PathBuf = root_dir.join(TLS_CACHE_DIR);
            create_private_dir_all_0700_sync(&cache_dir)
                .context(format!("TLS cache dir invalid: {}", cache_dir.display()))?;

            let domains = build_acme_domains(cfg)?;
            let directory_url = cfg.tls.acme_directory_url.clone();
            let contact_email = cfg.tls.acme_email.clone();

            match cfg.tls.acme_challenge {
                TlsAcmeChallenge::TlsAlpn01 => {
                    info!(
                        "TLS mode: acme (TLS-ALPN-01) – directory_url={}, cache_dir={}, domains={:?}, contact_email={:?}",
                        directory_url,
                        cache_dir.display(),
                        domains,
                        contact_email,
                    );

                    Ok(server::TlsConfig::AcmeTlsAlpn01 {
                        directory_url,
                        cache_dir,
                        domains,
                        contact_email,
                    })
                }

                TlsAcmeChallenge::Dns01 => {
                    info!(
                        "TLS mode: acme (DNS-01) – directory_url={}, cache_dir={}, domains={:?}, contact_email={:?}, acme_dns_api_base={:?}",
                        directory_url,
                        cache_dir.display(),
                        domains,
                        contact_email,
                        cfg.tls.acme_dns_api_base.clone(),
                    );

                    Ok(server::TlsConfig::AcmeDns01 {
                        directory_url,
                        cache_dir,
                        domains,
                        contact_email,
                        acme_dns_api_base: cfg.tls.acme_dns_api_base.clone(),
                    })
                }

                TlsAcmeChallenge::Http01 => Err(CliError::InvalidArgs(
                    "HTTP-01 is not supported yet. \
                     Use --tls-acme-challenge=tls-alpn-01 or dns-01."
                        .to_string(),
                )),
            }
        }
    }
}

fn build_acme_domains(cfg: &AppConfig) -> Result<Vec<String>, CliError> {
    let mut domains: Vec<String> = cfg
        .tls
        .sans
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    if let Some(ref host) = cfg.network.host
        && !host.trim().is_empty()
    {
        domains.push(host.clone());
    }

    // Dedup while preserving order.
    let mut seen = std::collections::BTreeSet::new();
    domains.retain(|d| seen.insert(d.clone()));

    if domains.is_empty() {
        return Err(CliError::InvalidArgs(
            "ACME mode requires at least one domain. \
Provide --tls-san and/or --net-host (or NET_HOST)."
                .to_string(),
        ));
    }

    Ok(domains)
}

fn build_auth_cfgs(
    cfg: &AppConfig,
) -> Result<
    (
        middleware::trusted_header_auth::ForwardAuthConfig,
        middleware::trusted_forwarded_for::TrustedForwardedForConfig,
        middleware::oidc::OidcConfig,
    ),
    CliError,
> {
    let auth_method: AuthenticationMethod = cfg.auth.method;

    let header_name_str = cfg.auth.trusted_header_name.as_str();
    let trusted_header_name = parse_header_name(header_name_str, "trusted header")?;

    let trusted_proxy: Option<IpAddr> = cfg.auth.trusted_proxy;

    let forward_auth_cfg = middleware::trusted_header_auth::ForwardAuthConfig {
        method: auth_method,
        trusted_header_name,
        trusted_proxy,
    };

    match auth_method {
        AuthenticationMethod::ForwardAuth => {
            let proxy = cfg
                .auth
                .trusted_proxy
                .expect("auth.validate() should guarantee trusted_proxy is Some for ForwardAuth");
            info!(
                "Authentication: forward_auth (trusted header='{}', proxy={})",
                header_name_str, proxy
            );
        }
        AuthenticationMethod::UsernamePassword => {
            info!("Authentication method: username_password");
        }
        AuthenticationMethod::Oidc => {
            info!("Authentication method: oidc");
        }
    }

    let fwd_enabled = cfg.auth.trusted_forwarded_for;
    let fwd_header_str = cfg.auth.trusted_forwarded_for_name.as_str();
    let fwd_header_name = parse_header_name(fwd_header_str, "forwarded-for header")?;

    let forward_for_cfg = middleware::trusted_forwarded_for::TrustedForwardedForConfig {
        enabled: fwd_enabled,
        header_name: fwd_header_name,
        trusted_proxy,
    };

    if fwd_enabled && let Some(t) = trusted_proxy {
        info!("Trusted FORWARDED-FOR enabled: header='{fwd_header_str}', trusted_proxy={t}");
    }

    let oidc_cfg = middleware::oidc::OidcConfig {
        enabled: cfg.auth.method == AuthenticationMethod::Oidc,
        host_port: build_host_port(&cfg.network.host, cfg.network.listen_port),
        issuer: cfg.auth.oidc_issuer.clone(),
        client_id: cfg.auth.oidc_client_id.clone(),
        client_secret: cfg.auth.oidc_client_secret.clone(),
    };

    Ok((forward_auth_cfg, forward_for_cfg, oidc_cfg))
}

fn parse_header_name(name: &str, label: &str) -> Result<HeaderName, CliError> {
    HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| CliError::InvalidArgs(format!("Invalid {label} name '{name}': {e}")))
}

fn create_runtime() -> Result<tokio::runtime::Runtime, CliError> {
    tokio::runtime::Runtime::new().map_err(|e| {
        error!("Failed to create Tokio runtime: {e}");
        CliError::RuntimeError(format!("Failed to start Tokio runtime: {e}"))
    })
}

fn strip_scheme_and_path(mut s: &str) -> &str {
    // If it looks like a URL, drop scheme://
    if let Some(rest) = s.strip_prefix("http://") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("https://") {
        s = rest;
    }

    // Drop any path/query/fragment
    if let Some(i) = s.find(['/', '?', '#']) {
        s = &s[..i];
    }

    s
}

fn build_host_port(host: &Option<String>, port: u16) -> Option<String> {
    let raw = host.as_deref().unwrap_or("localhost");
    let raw = raw.trim();
    if raw.is_empty() {
        return Some(format!("localhost:{port}"));
    }

    // Normalize to "authority-ish" (no scheme, no path)
    let authority = strip_scheme_and_path(raw);

    // If already bracketed IPv6 like "[::1]" or "[::1]:1234"
    if authority.starts_with('[') {
        // If it already has a port ("]:<digits>"), keep host part and override port.
        // Find closing bracket
        if let Some(end) = authority.find(']') {
            let host_part = &authority[..=end]; // includes closing ]
            return Some(format!("{host_part}:{port}"));
        }
        // Malformed bracket, fall back
        return Some(format!("localhost:{port}"));
    }

    // If it contains ':' now, it might be:
    // - IPv6 literal like "::1"  (needs brackets)
    // - host:port like "localhost:3000" (NOT IPv6)
    //
    // Heuristic:
    // - If it has 2+ colons, it's IPv6 -> bracket it.
    // - If it has exactly 1 colon, assume host:port -> take host part and override port.
    let colon_count = authority.matches(':').count();

    if colon_count >= 2 {
        // IPv6 literal
        return Some(format!("[{authority}]:{port}"));
    }

    if colon_count == 1 {
        // Probably "host:port" (domain names do not legally contain ':')
        let host_only = authority.split(':').next().unwrap_or(authority);
        return Some(format!("{host_only}:{port}"));
    }

    // Plain hostname / IPv4
    Some(format!("{authority}:{port}"))
}

#[test]
fn plan_serve_builds_expected_db_url_and_addr() {
    // Build a minimal config (fill in fields as needed for your structs)
    let cfg = AppConfig {
        network: crate::config::NetworkConfig {
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 3001,
            host: None,
            // ..other fields
        },
        database: crate::config::DatabaseConfig {
            url: None, /* .. */
            sqlite_path: "sqlite3".to_string(),
        },
        session: crate::config::SessionConfig {
            expiry_seconds: 3600,
            check_seconds: 60,
            // ..
        },
        auth: crate::config::AuthConfig {
            // ensure this matches validation expectations
            // ..
            ..Default::default()
        },
        tls: crate::config::TlsConfig {
            mode: crate::config::TlsMode::None,
            // ..
            ..Default::default()
        },
    };

    let root = std::path::PathBuf::from("/tmp/axum-dev-test");
    let plan = plan_serve(&cfg, &root).expect("plan_serve should succeed");

    assert_eq!(plan.addr.to_string(), "127.0.0.1:3001");
    assert!(plan.db_url.contains("sqlite://"));
    assert!(plan.db_url.contains("data.db"));
}
