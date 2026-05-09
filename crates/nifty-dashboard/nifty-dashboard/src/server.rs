use crate::{
    config::AuthConfig,
    middleware::{
        oidc::{OidcConfig, build_oidc_auth_layer},
        trusted_forwarded_for::TrustedForwardedForConfig,
        trusted_header_auth::ForwardAuthConfig,
    },
    prelude::*,
    routes::router,
    tls::{
        dns::{AcmeDnsProvider, obtain_certificate_with_dns01},
        generate::{
            ensure_rustls_crypto_provider, load_or_generate_self_signed, renew_self_signed_loop,
        },
        self_signed_cache::{delete_cached_pair, read_private_tls_file, read_tls_file},
    },
    util::write_files::{atomic_write_file_0600, create_private_dir_all_0700},
};
use anyhow::Context;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use futures_util::StreamExt;
use rustls::ServerConfig as RustlsServerConfig;
use sqlx::{ConnectOptions, SqlitePool, sqlite::SqliteConnectOptions};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::task::AbortHandle;
use tokio_rustls_acme::{AcmeConfig, caches::DirCache};
use tower_sessions::{
    Expiry, SessionManagerLayer, cookie::time::Duration as CookieDuration,
    session_store::ExpiredDeletion,
};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::warn;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub auth_config: AuthConfig,
    pub config_changed_tx: tokio::sync::broadcast::Sender<()>,
    pub config_boot_sha: String,
    pub config_boot_values: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub enum TlsConfig {
    /// Plain HTTP, no TLS.
    Http,
    /// Rustls with certificate and key loaded from PEM files.
    RustlsFiles {
        cert_path: PathBuf,
        key_path: PathBuf,
    },
    /// Self-signed TLS, generated at startup.
    ///
    /// If `cache_dir` is Some, certificates are stored/reused there.
    /// If `cache_dir` is None, certificates are ephemeral (in-memory only).
    SelfSigned {
        cache_dir: Option<PathBuf>,
        sans: Vec<String>,
        leaf_valid_secs: u32,
        ca_valid_secs: u32,
    },
    /// ACME (Let's Encrypt or other CA) via TLS-ALPN-01.
    ///
    /// Certificates and account data are stored in `cache_dir`.
    AcmeTlsAlpn01 {
        directory_url: String,
        cache_dir: PathBuf,
        domains: Vec<String>,
        contact_email: Option<String>,
    },

    /// ACME via **DNS-01**, using a DNS provider (e.g. acme-dns).
    ///
    /// Certificates and account data are stored in `cache_dir`.
    AcmeDns01 {
        directory_url: String,
        cache_dir: PathBuf,
        domains: Vec<String>,
        contact_email: Option<String>,
        acme_dns_api_base: String,
    },
}

/// Run the HTTP server until shutdown.
pub async fn run(
    addr: SocketAddr,
    forward_auth_cfg: ForwardAuthConfig,
    forward_for_cfg: TrustedForwardedForConfig,
    oidc_cfg: OidcConfig,
    db_url: String,
    session_secure: bool,
    session_expiry_secs: u64,
    session_check_secs: u64,
    tls_config: TlsConfig,
    auth_config: AuthConfig,
) -> anyhow::Result<()> {
    log_startup(&db_url)?;

    // Database pool and migration
    let db = init_db(&db_url).await?;

    // Session store + background deletion task
    let (session_layer, deletion_task) = init_sessions(
        db.clone(),
        session_secure,
        session_expiry_secs,
        session_check_secs,
    )
    .await?;
    let deletion_abort = deletion_task.abort_handle();

    let oidc_auth_layer = build_oidc_auth_layer(&oidc_cfg).await?;
    debug_assert!(!oidc_cfg.enabled || oidc_auth_layer.is_some());

    // Config file watcher
    let (config_changed_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let config_boot_sha = crate::config_watcher::read_boot_sha().await;
    info!("config boot SHA: {}", config_boot_sha);
    let config_boot_values = {
        // Read the boot-time config snapshot from /run/ (written by nifty-config-sha.service).
        // Falls back to current config file if snapshot doesn't exist (dev/non-NixOS).
        let boot_snapshot_path = std::env::var("NIFTY_CONFIG_BOOT_SHA_FILE")
            .map(|p| {
                std::path::PathBuf::from(p)
                    .parent()
                    .unwrap_or(std::path::Path::new("/run/nifty-filter"))
                    .join("config-boot-snapshot")
            })
            .unwrap_or_else(|_| {
                std::path::PathBuf::from("/run/nifty-filter/config-boot-snapshot")
            });
        let contents = match tokio::fs::read_to_string(&boot_snapshot_path).await {
            Ok(c) => c,
            Err(_) => {
                // Fallback: read current config file
                let path = crate::config_watcher::config_file_path();
                tokio::fs::read_to_string(&path).await.unwrap_or_default()
            }
        };
        crate::routes::status::parse_hcl_to_json(&contents).ok()
    };
    crate::config_watcher::spawn_config_watcher(config_changed_tx.clone());

    // Shared state + router
    let state = AppState {
        db,
        auth_config,
        config_changed_tx,
        config_boot_sha,
        config_boot_values,
    };
    let app = build_app(
        forward_auth_cfg,
        forward_for_cfg,
        oidc_cfg,
        oidc_auth_layer,
        state,
        session_layer,
    );

    ensure_rustls_crypto_provider();

    // Serve based on TLS mode
    match tls_config {
        TlsConfig::Http => serve_http(addr, app, deletion_abort).await?,

        TlsConfig::RustlsFiles {
            cert_path,
            key_path,
        } => serve_rustls_files(addr, app, deletion_abort, cert_path, key_path).await?,

        TlsConfig::SelfSigned {
            cache_dir,
            mut sans,
            leaf_valid_secs,
            ca_valid_secs,
        } => {
            if sans.is_empty() {
                sans.push("localhost".to_string());
            }
            serve_self_signed(
                addr,
                app,
                deletion_abort,
                cache_dir,
                sans,
                leaf_valid_secs,
                ca_valid_secs,
            )
            .await?
        }

        TlsConfig::AcmeTlsAlpn01 {
            directory_url,
            cache_dir,
            domains,
            contact_email,
        } => {
            serve_acme_tls_alpn01(
                addr,
                app,
                deletion_abort,
                directory_url,
                cache_dir,
                domains,
                contact_email,
            )
            .await?
        }

        TlsConfig::AcmeDns01 {
            directory_url,
            cache_dir,
            domains,
            contact_email,
            acme_dns_api_base,
        } => {
            serve_acme_dns01(
                addr,
                app,
                deletion_abort,
                directory_url,
                cache_dir,
                domains,
                contact_email,
                acme_dns_api_base,
            )
            .await?
        }
    }

    // Make sure the background deletion task finishes cleanly.
    deletion_task.await??;

    Ok(())
}

fn log_startup(db_url: &str) -> anyhow::Result<()> {
    let cwd =
        std::env::current_dir().with_context(|| "failed to determine current working directory")?;
    debug!(
        "server::run starting; cwd='{}', db_url='{}'",
        cwd.display(),
        db_url
    );
    Ok(())
}

async fn init_db(db_url: &str) -> anyhow::Result<SqlitePool> {
    use std::str::FromStr;
    warn!("foo");
    let connect_opts = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .log_statements(tracing::log::LevelFilter::Trace)
        .log_slow_statements(
            tracing::log::LevelFilter::Warn,
            std::time::Duration::from_millis(100),
        );

    let db: SqlitePool = SqlitePool::connect_with(connect_opts).await?;
    debug!("Loaded database connection pool. DATABASE_URL={db_url}");
    sqlx::migrate!().run(&db.clone()).await?;
    debug!("sqlx migration complete");
    Ok(db)
}

async fn init_sessions(
    db: SqlitePool,
    session_secure: bool,
    session_expiry_secs: u64,
    session_check_secs: u64,
) -> anyhow::Result<(
    SessionManagerLayer<SqliteStore>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
)> {
    // Session store
    let session_store = SqliteStore::new(db.clone());
    session_store.migrate().await?;

    let deletion_task = start_session_deletion_task(session_store.clone(), session_check_secs);

    // Convert the CLI/env-specified seconds into a cookie::time::Duration
    let session_expiry = CookieDuration::seconds(session_expiry_secs as i64);

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(session_secure)
        .with_expiry(Expiry::OnInactivity(session_expiry));

    Ok((session_layer, deletion_task))
}

fn start_session_deletion_task(
    session_store: SqliteStore,
    session_check_secs: u64,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    tokio::task::spawn(async move {
        session_store
            .continuously_delete_expired(core::time::Duration::from_secs(session_check_secs))
            .await
            .map_err(Into::into)
    })
}

fn build_app(
    forward_auth_cfg: ForwardAuthConfig,
    forward_for_cfg: TrustedForwardedForConfig,
    oidc_cfg: OidcConfig,
    oidc_auth_layer: Option<axum_oidc::OidcAuthLayer<axum_oidc::EmptyAdditionalClaims>>,
    state: AppState,
    session_layer: SessionManagerLayer<SqliteStore>,
) -> axum::Router {
    router(
        forward_auth_cfg,
        forward_for_cfg,
        oidc_cfg,
        oidc_auth_layer,
        state.clone(),
    )
    .layer(session_layer)
    .with_state(state)
    .into()
}

async fn serve_http(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
) -> anyhow::Result<()> {
    use std::io;

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(http_bind_permission_denied(addr));
        }
        Err(e) => return Err(anyhow::anyhow!("Failed to bind to {addr}: {e}")),
    };

    let bound_addr = listener.local_addr()?;
    debug!("listening on http://{bound_addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(deletion_abort, None))
    .await?;

    Ok(())
}

async fn serve_rustls_files(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    cert_path: PathBuf,
    key_path: PathBuf,
) -> anyhow::Result<()> {
    info!(
        "loading TLS certificate from '{}' and key from '{}'",
        cert_path.display(),
        key_path.display()
    );

    // Enforce permissions + readability ourselves (and avoid any later path-based reads).
    let cert_pem = read_tls_file(&cert_path).await?;
    let key_pem = read_private_tls_file(&key_path).await?;

    let rustls_config = RustlsConfig::from_pem(cert_pem, key_pem).await?;

    info!("listening on https://{addr}");

    serve_with_handle(addr, deletion_abort, |handle| {
        axum_server::bind_rustls(addr, rustls_config)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

async fn serve_acme_tls_alpn01(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    directory_url: String,
    cache_dir: PathBuf,
    domains: Vec<String>,
    contact_email: Option<String>,
) -> anyhow::Result<()> {
    use std::sync::Arc;

    create_private_dir_all_0700(&cache_dir)
        .await
        .map_err(|e| anyhow::anyhow!("TLS cache dir invalid: {e:#}"))?;

    info!(
        "Starting ACME TLS (tls-alpn-01) – directory_url='{}', cache_dir='{}', domains={:?}, contact_email={:?}",
        directory_url,
        cache_dir.display(),
        domains,
        contact_email,
    );

    let mut state = {
        let mut cfg = AcmeConfig::new(domains.clone())
            .cache(DirCache::new(cache_dir.clone()))
            .directory(directory_url.clone());

        if let Some(ref email) = contact_email
            && !email.is_empty()
        {
            cfg = cfg.contact([format!("mailto:{email}")]);
        }

        cfg.state()
    };

    let rustls_config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(state.resolver());

    let acceptor = state.axum_acceptor(Arc::new(rustls_config));

    tokio::spawn(async move {
        while let Some(res) = state.next().await {
            match res {
                Ok(ev) => tracing::info!("acme event: {:?}", ev),
                Err(err) => tracing::error!("acme error: {:?}", err),
            }
        }
    });

    info!("listening on https://{addr} (ACME)");

    serve_with_handle(addr, deletion_abort, |handle| {
        axum_server::bind(addr)
            .handle(handle)
            .acceptor(acceptor)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

async fn serve_acme_dns01(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    directory_url: String,
    cache_dir: PathBuf,
    domains: Vec<String>,
    contact_email: Option<String>,
    acme_dns_api_base: String,
) -> anyhow::Result<()> {
    create_private_dir_all_0700(&cache_dir)
        .await
        .map_err(|e| anyhow::anyhow!("TLS cache dir invalid: {e:#}"))?;

    info!(
        "Starting ACME TLS (dns-01) – directory_url='{}', cache_dir='{}', domains={:?}, contact_email={:?}",
        directory_url,
        cache_dir.display(),
        domains,
        contact_email,
    );

    let (cert_pem, key_pem) = load_or_request_dns01_cert(
        &directory_url,
        &cache_dir,
        &domains,
        contact_email.as_deref(),
        &acme_dns_api_base,
    )
    .await?;

    let rustls_config = RustlsConfig::from_pem(cert_pem, key_pem).await?;

    info!("listening on https://{addr} (ACME dns-01)");

    serve_with_handle(addr, deletion_abort, |handle| {
        axum_server::bind_rustls(addr, rustls_config)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

/// Common “axum_server + Handle + shutdown_signal” pattern used by HTTPS modes.
async fn serve_with_handle<E, Fut, Mk>(
    addr: SocketAddr,
    deletion_abort: tokio::task::AbortHandle,
    mk_server: Mk,
) -> anyhow::Result<()>
where
    E: std::fmt::Display,
    Fut: std::future::Future<Output = Result<(), E>> + Send,
    Mk: FnOnce(axum_server::Handle) -> Fut,
{
    // Create a handle for graceful shutdown
    let handle = axum_server::Handle::new();

    // Spawn the shutdown handler that will:
    //  - abort the deletion task
    //  - call handle.graceful_shutdown(...)
    let shutdown_task = tokio::spawn(shutdown_signal(deletion_abort, Some(handle.clone())));

    let server = mk_server(handle.clone());

    if let Err(e) = server.await {
        let msg = e.to_string();
        if msg.contains("Permission denied") {
            return Err(https_bind_permission_denied(addr, &msg));
        }
        return Err(anyhow::anyhow!("HTTPS server failed on {addr}: {e}"));
    }

    // Make sure the shutdown task has finished (and bubble up any errors)
    shutdown_task.await?;

    Ok(())
}

async fn serve_self_signed(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    cache_dir: Option<PathBuf>,
    sans: Vec<String>,
    leaf_valid_secs: u32,
    ca_valid_secs: u32,
) -> anyhow::Result<()> {
    let tls_material = load_or_generate_self_signed(
        cache_dir.clone(),
        sans.clone(),
        leaf_valid_secs,
        ca_valid_secs,
    )
    .await?;

    let rustls_config = RustlsConfig::from_pem(
        tls_material.chain_pem.clone(),
        tls_material.leaf_key_pem.clone(),
    )
    .await?;

    // Renew at ~80% lifetime: margin = 20% validity, capped at 10 minutes.
    // Also ensure margin is strictly less than validity.
    let validity = Duration::from_secs(leaf_valid_secs as u64);
    let mut renew_margin =
        Duration::from_secs((leaf_valid_secs as u64) / 5).max(Duration::from_secs(1));
    renew_margin = renew_margin.min(Duration::from_secs(600));
    if renew_margin >= validity {
        // If validity is tiny, renew_margin must be < validity or we’ll loop.
        renew_margin = validity
            .saturating_sub(Duration::from_secs(1))
            .max(Duration::from_secs(1));
    }

    tokio::spawn(renew_self_signed_loop(
        rustls_config.clone(),
        cache_dir,
        sans,
        leaf_valid_secs,
        renew_margin,
        tls_material.leaf_cert_pem,
    ));

    info!("listening on https://{addr} (self-signed)");

    serve_with_handle(addr, deletion_abort, |handle| {
        axum_server::bind_rustls(addr, rustls_config)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

async fn load_or_request_dns01_cert(
    directory_url: &str,
    cache_dir: &std::path::Path,
    domains: &[String],
    contact_email: Option<&str>,
    acme_dns_api_base: &str,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let cert_path = cache_dir.join("acme-dns01-cert.pem");
    let key_path = cache_dir.join("acme-dns01-key.pem");

    let cert_exists = cert_path.exists();
    let key_exists = key_path.exists();

    if cert_exists && key_exists {
        let cert_pem = read_tls_file(&cert_path).await?;
        let key_pem = read_private_tls_file(&key_path).await?;

        match pem_cert_is_valid_now(&cert_pem) {
            Ok(true) => {
                info!(
                    "Using cached ACME dns-01 certificate from '{}' and key from '{}'",
                    cert_path.display(),
                    key_path.display()
                );
                return Ok((cert_pem, key_pem));
            }
            Ok(false) => {
                info!(
                    "Cached ACME dns-01 cert at '{}' is expired/invalid; requesting a new one",
                    cert_path.display()
                );
                delete_cached_pair(&cert_path, &key_path).await?;
            }
            Err(err) => {
                info!(
                    "Failed to parse cached ACME dns-01 cert '{}': {err}; requesting a new one",
                    cert_path.display()
                );
                delete_cached_pair(&cert_path, &key_path).await?;
            }
        }
    } else if cert_exists || key_exists {
        info!(
            "Cached ACME dns-01 cert/key incomplete; deleting and requesting a new one (cert_exists={}, key_exists={})",
            cert_exists, key_exists
        );
        delete_cached_pair(&cert_path, &key_path).await?;
    }

    info!(
        "Requesting new ACME dns-01 certificate (directory_url='{}', domains={:?})",
        directory_url, domains
    );

    let dns_provider = AcmeDnsProvider::from_cache(acme_dns_api_base, cache_dir)
        .await?
        .into_shared();

    let (cert_pem, key_pem) = obtain_certificate_with_dns01(
        directory_url,
        contact_email,
        domains,
        dns_provider.as_ref(),
        cache_dir,
    )
    .await
    .map_err(|e| anyhow::anyhow!("ACME dns-01 flow failed: {e:#}"))?;

    // 🔐 Persist with secure perms atomically.
    atomic_write_file_0600(&cert_path, &cert_pem)
        .await
        .with_context(|| {
            format!(
                "failed to write ACME dns-01 certificate to '{}'",
                cert_path.display()
            )
        })?;
    atomic_write_file_0600(&key_path, &key_pem)
        .await
        .with_context(|| {
            format!(
                "failed to write ACME dns-01 key to '{}'",
                key_path.display()
            )
        })?;

    info!(
        "Wrote ACME dns-01 certificate to '{}' and key to '{}'",
        cert_path.display(),
        key_path.display()
    );

    Ok((cert_pem, key_pem))
}

fn pem_cert_is_valid_now(pem_bytes: &[u8]) -> anyhow::Result<bool> {
    use rustls_pemfile::certs as load_pem_certs;
    use x509_parser::prelude::*;

    let mut slice: &[u8] = pem_bytes;
    let mut iter = load_pem_certs(&mut slice);

    // Option<Result<CertificateDer<'static>, io::Error>>
    let der = iter
        .next()
        .transpose()
        .context("failed to decode PEM cert")?
        .context("PEM contained no certificates")?;

    let (_rem, x509) = parse_x509_certificate(der.as_ref())
        .map_err(|e| anyhow::anyhow!("x509 parse error: {e}"))?;

    let validity = x509.validity();
    let now = ASN1Time::now();
    Ok(validity.is_valid_at(now))
}

/// Shutdown signal for graceful shutdown on Ctrl+C / SIGTERM.
async fn shutdown_signal(deletion_task_abort_handle: AbortHandle, handle: Option<Handle>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received; starting graceful shutdown");

    // Stop the background deletion task
    deletion_task_abort_handle.abort();

    // If we are running behind axum_server, trigger graceful shutdown there too
    if let Some(handle) = handle {
        handle.graceful_shutdown(Some(Duration::from_secs(1)));
    }

    // Force exit after deadline — axum::serve graceful shutdown waits
    // indefinitely for open connections (e.g. SSE streams).
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        info!("graceful shutdown deadline reached; forcing exit");
        std::process::exit(0);
    });
}

fn privileged_port_hint() -> &'static str {
    "This usually means you're trying to bind to a privileged port \
(below 1024, such as 80 or 443) without sufficient privileges.\n\
Either:\n  - run with appropriate permissions (root or CAP_NET_BIND_SERVICE), or\n  - listen on a higher port (e.g. 3000) and front it with a reverse proxy."
}

fn http_bind_permission_denied(addr: SocketAddr) -> anyhow::Error {
    anyhow::anyhow!(
        "Failed to bind to {addr}: Permission denied (os error 13).\n{}",
        privileged_port_hint()
    )
}

fn https_bind_permission_denied(addr: SocketAddr, msg: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "Failed to start HTTPS listener on {addr}: {msg}\n{}",
        privileged_port_hint()
    )
}
