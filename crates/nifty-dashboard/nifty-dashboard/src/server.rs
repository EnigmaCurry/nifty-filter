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
        http_redirect::HttpRedirectAcceptor,
        self_signed_cache::{delete_cached_pair, read_private_tls_file, read_tls_file},
    },
    util::write_files::{atomic_write_file_0600, create_private_dir_all_0700},
};
use anyhow::Context;
use axum_server::{Handle, accept::DefaultAcceptor, tls_rustls::{RustlsAcceptor, RustlsConfig}};
use futures_util::StreamExt;
use rustls::ServerConfig as RustlsServerConfig;
use rustls::server::WebPkiClientVerifier;
use sqlx::{ConnectOptions, SqlitePool, sqlite::SqliteConnectOptions};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::task::AbortHandle;
use tokio_rustls_acme::{AcmeConfig, caches::DirCache};
use tower_sessions::{
    Expiry, SessionManagerLayer, cookie::time::Duration as CookieDuration,
    session_store::ExpiredDeletion,
};
use tower_sessions_sqlx_store::SqliteStore;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub auth_config: AuthConfig,
    pub config_changed_tx: tokio::sync::broadcast::Sender<()>,
    pub shutdown_tx: tokio::sync::broadcast::Sender<()>,
    pub config_boot_values: Option<serde_json::Value>,
    /// Shared HTTP client for outbound connections to services VM (Traefik).
    /// Uses system roots for CA verification + optional mTLS client cert.
    pub services_client: reqwest::Client,
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
    /// ACME (Let's Encrypt, Step-CA, or other CA) via TLS-ALPN-01.
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
    public_port: u16,
    forward_auth_cfg: ForwardAuthConfig,
    forward_for_cfg: TrustedForwardedForConfig,
    oidc_cfg: OidcConfig,
    db_url: String,
    session_secure: bool,
    session_expiry_secs: u64,
    session_check_secs: u64,
    tls_config: TlsConfig,
    auth_config: AuthConfig,
    client_cert_path: Option<PathBuf>,
    client_key_path: Option<PathBuf>,
    client_ca_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    // Install the ring crypto provider before any rustls usage
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

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

    // Shutdown broadcast channel — SSE clients receive "shutdown" before the server stops
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Shared state + router
    let services_client = build_services_client(
        client_cert_path.as_deref(),
        client_key_path.as_deref(),
    )?;
    let state = AppState {
        db,
        auth_config,
        config_changed_tx,
        shutdown_tx: shutdown_tx.clone(),
        config_boot_values,
        services_client,
    };
    let app = build_app(
        forward_auth_cfg,
        forward_for_cfg,
        oidc_cfg,
        oidc_auth_layer,
        state,
        session_layer,
    );

    // Serve based on TLS mode
    match tls_config {
        TlsConfig::Http => serve_http(addr, app, deletion_abort, shutdown_tx).await?,

        TlsConfig::RustlsFiles {
            cert_path,
            key_path,
        } => {
            let ca = client_ca_path.clone().context("--tls-client-ca is required for mTLS")?;
            serve_rustls_files(addr, app, deletion_abort, shutdown_tx, public_port, cert_path, key_path, ca).await?
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
                shutdown_tx,
                public_port,
                directory_url,
                cache_dir,
                domains,
                contact_email,
                client_ca_path.clone(),
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
                shutdown_tx,
                public_port,
                directory_url,
                cache_dir,
                domains,
                contact_email,
                acme_dns_api_base,
                client_ca_path.clone().context("--tls-client-ca is required for mTLS")?,
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
    let connect_opts = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .log_statements(tracing::log::LevelFilter::Trace)
        .log_slow_statements(
            tracing::log::LevelFilter::Warn,
            std::time::Duration::from_secs(1),
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

/// Build a client certificate verifier using only the Step-CA root cert.
/// The CA cert path comes from TLS_CLIENT_CA env var or the --tls-client-ca flag.
/// Only clients with certs signed by this specific CA are accepted.
fn build_mtls_client_verifier(ca_cert_path: &std::path::Path) -> anyhow::Result<Arc<dyn rustls::server::danger::ClientCertVerifier>> {
    let ca_pem = std::fs::read(ca_cert_path)
        .with_context(|| format!("failed to read CA cert '{}'", ca_cert_path.display()))?;
    let mut root_store = rustls::RootCertStore::empty();
    let certs = rustls_pemfile::certs(&mut &ca_pem[..])
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse CA cert PEM")?;
    for cert in certs {
        root_store.add(cert).context("failed to add CA cert to root store")?;
    }
    if root_store.is_empty() {
        anyhow::bail!("no certificates found in CA cert file '{}'", ca_cert_path.display());
    }
    let verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .context("failed to build client certificate verifier")?;
    Ok(verifier)
}

/// Build a shared reqwest::Client for outbound connections to the services VM.
/// Uses system roots for CA verification + optional mTLS client cert.
fn build_services_client(
    client_cert: Option<&std::path::Path>,
    client_key: Option<&std::path::Path>,
) -> anyhow::Result<reqwest::Client> {
    let mut root_store = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        root_store.add(cert).context("failed to add system root cert")?;
    }

    let tls_config = if let (Some(cert_path), Some(key_path)) = (client_cert, client_key) {
        let cert_data = std::fs::read(cert_path)
            .with_context(|| format!("failed to read client cert '{}'", cert_path.display()))?;
        let key_data = std::fs::read(key_path)
            .with_context(|| format!("failed to read client key '{}'", key_path.display()))?;
        let certs = rustls_pemfile::certs(&mut &cert_data[..])
            .collect::<Result<Vec<_>, _>>()
            .context("failed to parse client cert PEM")?;
        let key = rustls_pemfile::private_key(&mut &key_data[..])
            .context("failed to parse client key PEM")?
            .context("no private key found in client key PEM")?;
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, key)
            .context("failed to configure client auth cert for outbound connections")?
    } else {
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to build services HTTP client")
}

/// Build a `RustlsConfig` from PEM cert/key with mTLS client verification enabled.
async fn rustls_config_with_mtls(
    cert_pem: Vec<u8>,
    key_pem: Vec<u8>,
    ca_cert_path: &std::path::Path,
) -> anyhow::Result<RustlsConfig> {
    let client_verifier = build_mtls_client_verifier(ca_cert_path)?;

    let certs = rustls_pemfile::certs(&mut &cert_pem[..])
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse certificate PEM")?;
    let key = rustls_pemfile::private_key(&mut &key_pem[..])
        .context("failed to parse private key PEM")?
        .context("no private key found in PEM")?;

    let server_config = RustlsServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)
        .context("failed to build server TLS config with client auth")?;

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}

async fn serve_http(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
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
    .with_graceful_shutdown(shutdown_signal(deletion_abort, None, shutdown_tx))
    .await?;

    Ok(())
}

async fn serve_rustls_files(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    public_port: u16,
    cert_path: PathBuf,
    key_path: PathBuf,
    client_ca_path: PathBuf,
) -> anyhow::Result<()> {
    info!(
        "loading TLS certificate from '{}' and key from '{}'",
        cert_path.display(),
        key_path.display()
    );

    // Enforce permissions + readability ourselves (and avoid any later path-based reads).
    let cert_pem = read_tls_file(&cert_path).await?;
    let key_pem = read_private_tls_file(&key_path).await?;

    let rustls_config = rustls_config_with_mtls(cert_pem, key_pem, &client_ca_path).await?;

    info!("listening on https://{addr} (manual certs, mTLS)");

    let acceptor = RustlsAcceptor::new(rustls_config)
        .acceptor(HttpRedirectAcceptor::new(DefaultAcceptor::new(), public_port));

    serve_with_handle(addr, deletion_abort, shutdown_tx, |handle| {
        axum_server::bind(addr)
            .acceptor(acceptor)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

async fn serve_acme_tls_alpn01(
    addr: SocketAddr,
    app: axum::Router,
    deletion_abort: tokio::task::AbortHandle,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    public_port: u16,
    directory_url: String,
    cache_dir: PathBuf,
    domains: Vec<String>,
    contact_email: Option<String>,
    client_ca_path: Option<PathBuf>,
) -> anyhow::Result<()> {
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
        // Build a rustls ClientConfig with native roots (includes Step-CA root
        // from security.pki.certificateFiles). tokio-rustls-acme defaults to
        // webpki-roots which only has public CAs.
        let mut acme_root_store = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = acme_root_store.add(cert);
        }
        let acme_client_config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(acme_root_store)
                .with_no_client_auth(),
        );

        let mut cfg = AcmeConfig::new(domains.clone())
            .client_tls_config(acme_client_config)
            .cache(DirCache::new(cache_dir.clone()))
            .directory(directory_url.clone());

        if let Some(ref email) = contact_email
            && !email.is_empty()
        {
            cfg = cfg.contact([format!("mailto:{email}")]);
        }

        cfg.state()
    };

    let client_verifier = build_mtls_client_verifier(
        client_ca_path.as_ref().context("--tls-client-ca is required for mTLS")?,
    )?;
    let rustls_config = RustlsServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_cert_resolver(state.resolver());

    let acme_acceptor = state.axum_acceptor(Arc::new(rustls_config));
    let acceptor = HttpRedirectAcceptor::new(acme_acceptor, public_port);

    tokio::spawn(async move {
        while let Some(res) = state.next().await {
            match res {
                Ok(ev) => tracing::info!("acme event: {:?}", ev),
                Err(err) => tracing::error!("acme error: {:?}", err),
            }
        }
    });

    info!("listening on https://{addr} (ACME)");

    serve_with_handle(addr, deletion_abort, shutdown_tx, |handle| {
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
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    public_port: u16,
    directory_url: String,
    cache_dir: PathBuf,
    domains: Vec<String>,
    contact_email: Option<String>,
    acme_dns_api_base: String,
    client_ca_path: PathBuf,
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

    let rustls_config = rustls_config_with_mtls(cert_pem, key_pem, &client_ca_path).await?;

    info!("listening on https://{addr} (ACME dns-01, mTLS)");

    let acceptor = RustlsAcceptor::new(rustls_config)
        .acceptor(HttpRedirectAcceptor::new(DefaultAcceptor::new(), public_port));

    serve_with_handle(addr, deletion_abort, shutdown_tx, |handle| {
        axum_server::bind(addr)
            .acceptor(acceptor)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
    })
    .await
}

/// Common “axum_server + Handle + shutdown_signal” pattern used by HTTPS modes.
async fn serve_with_handle<E, Fut, Mk>(
    addr: SocketAddr,
    deletion_abort: tokio::task::AbortHandle,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
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
    let shutdown_task = tokio::spawn(shutdown_signal(deletion_abort, Some(handle.clone()), shutdown_tx));

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
async fn shutdown_signal(
    deletion_task_abort_handle: AbortHandle,
    handle: Option<Handle>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) {
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

    // Notify SSE clients that the server is going away
    let _ = shutdown_tx.send(());

    // Give SSE clients a moment to receive the shutdown event
    tokio::time::sleep(Duration::from_millis(100)).await;

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
