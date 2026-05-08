use anyhow::Context;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, Issuer,
    KeyPair, KeyUsagePurpose,
};
use std::{
    sync::Once,
    time::{Duration, SystemTime},
};
use time::OffsetDateTime;
use tracing::info;

use crate::{
    tls::{
        cert_details::format_remaining_compact,
        self_signed_cache::{
            delete_cached_pair, inspect_self_signed_cert_pem, read_private_tls_file, read_tls_file,
            validate_leaf_signed_by_ca_cert_pem,
        },
    },
    util::write_files::{
        atomic_write_file_0600, create_private_dir_all_0700, validate_private_dir_0700,
    },
};

static INSTALL_RUSTLS_PROVIDER: Once = Once::new();
const CERT_FILE_NAME: &str = "self_signed_cert.pem";
const KEY_FILE_NAME: &str = "self_signed_key.pem";
const CA_CERT_FILE_NAME: &str = "self_signed_CA_cert.pem";
const CA_KEY_FILE_NAME: &str = "self_signed_CA_key.pem";

#[derive(Debug, Clone)]
pub struct TlsMaterial {
    /// What the server should present: leaf first, then any intermediates/roots.
    pub chain_pem: Vec<u8>,

    /// Private key corresponding to the leaf certificate.
    pub leaf_key_pem: Vec<u8>,

    /// Leaf certificate only (used for expiry timing + fingerprint logging).
    pub leaf_cert_pem: Vec<u8>,
}

impl TlsMaterial {
    #[allow(dead_code)]
    pub fn for_rustls(&self) -> (Vec<u8>, Vec<u8>) {
        (self.chain_pem.clone(), self.leaf_key_pem.clone())
    }
}

pub fn ensure_rustls_crypto_provider() {
    INSTALL_RUSTLS_PROVIDER.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install rustls crypto provider");
    });
}

#[derive(Debug, Clone)]
pub struct SelfSignedDn {
    pub organization: String,
    pub common_name: String,
}

impl SelfSignedDn {
    pub fn from_bin_name(bin: &str) -> Self {
        Self {
            organization: format!("{bin} self-signed authority"),
            common_name: bin.to_string(),
        }
    }
}

pub fn default_self_signed_dn() -> SelfSignedDn {
    SelfSignedDn::from_bin_name(env!("CARGO_BIN_NAME"))
}

pub fn default_self_signed_ca_dn() -> SelfSignedDn {
    let bin = env!("CARGO_BIN_NAME");
    SelfSignedDn {
        organization: format!("{bin} self-signed authority"),
        common_name: format!("{bin} local CA"),
    }
}

/// Backwards-compatible wrapper
pub fn generate_self_signed_with_validity(
    sans: Vec<String>,
    valid_secs: u32,
) -> Result<(Vec<u8>, Vec<u8>), rcgen::Error> {
    generate_self_signed_with_validity_and_dn(sans, valid_secs, &default_self_signed_dn())
}

/// New API: caller provides the DN.
pub fn generate_self_signed_with_validity_and_dn(
    sans: Vec<String>,
    valid_secs: u32,
    dn: &SelfSignedDn,
) -> Result<(Vec<u8>, Vec<u8>), rcgen::Error> {
    let mut params = CertificateParams::new(sans)?;

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(rcgen::DnType::OrganizationName, dn.organization.clone());
    distinguished_name.push(rcgen::DnType::CommonName, dn.common_name.clone());
    params.distinguished_name = distinguished_name;

    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + Duration::from_secs(valid_secs as u64);

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    Ok((
        cert.pem().into_bytes(),
        key_pair.serialize_pem().into_bytes(),
    ))
}

fn concat_pem_chain(leaf_pem: &[u8], ca_pem: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(leaf_pem.len() + ca_pem.len() + 1);
    out.extend_from_slice(leaf_pem);
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(ca_pem);
    out
}

pub async fn load_or_generate_self_signed(
    cache_dir: Option<std::path::PathBuf>,
    sans: Vec<String>,
    leaf_valid_secs: u32,
    ca_valid_secs: u32,
) -> anyhow::Result<TlsMaterial> {
    let leaf_dn = default_self_signed_dn();

    if let Some(dir) = cache_dir.as_ref() {
        create_private_dir_all_0700(dir)
            .await
            .map_err(|e| anyhow::anyhow!("TLS cache dir invalid: {e:#}"))?;

        let ca_dn = default_self_signed_ca_dn();
        let ca_cert_path = dir.join(CA_CERT_FILE_NAME);
        let ca_key_path = dir.join(CA_KEY_FILE_NAME);

        let (ca_cert_pem, ca_key_pem) = {
            let ca_cert_exists = tokio::fs::try_exists(&ca_cert_path).await?;
            let ca_key_exists = tokio::fs::try_exists(&ca_key_path).await?;
            // 1) Load or create CA
            if ca_cert_exists && ca_key_exists {
                let cert_pem = read_tls_file(&ca_cert_path).await?;
                let key_pem = read_private_tls_file(&ca_key_path).await?;

                match crate::tls::self_signed_cache::validate_local_ca_cert_pem(&cert_pem, &ca_dn) {
                    Ok(()) => {
                        let fp = sha256_fingerprint_first_cert_pem(&cert_pem)
                            .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));
                        if let Ok(details) = inspect_self_signed_cert_pem(&cert_pem) {
                            info!(
                                "Loaded cached local CA certificate (sha256_fingerprint={}, expires {}, remaining {})",
                                fp, details.not_after, details.remaining_human
                            );
                        } else {
                            info!(
                                "Loaded cached local CA certificate (sha256_fingerprint={})",
                                fp
                            );
                        }
                        (cert_pem, key_pem)
                    }
                    Err(err) => {
                        info!("Cached local CA invalid: {err}; deleting and regenerating");
                        delete_cached_pair(&ca_cert_path, &ca_key_path).await?;
                        let (cert_pem, key_pem) =
                            generate_local_ca_with_validity_and_dn(ca_valid_secs, &ca_dn)
                                .map_err(|e| anyhow::anyhow!("failed to generate local CA: {e}"))?;
                        atomic_write_file_0600(&ca_cert_path, &cert_pem).await?;
                        atomic_write_file_0600(&ca_key_path, &key_pem).await?;

                        let fp = sha256_fingerprint_first_cert_pem(&cert_pem)
                            .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));
                        info!(
                            "Generated new local CA certificate (sha256_fingerprint={})",
                            fp
                        );

                        (cert_pem, key_pem)
                    }
                }
            } else {
                if ca_cert_exists || ca_key_exists {
                    info!(
                        "Cached local CA incomplete; deleting and regenerating (cert_exists={}, key_exists={})",
                        ca_cert_exists, ca_key_exists
                    );
                    delete_cached_pair(&ca_cert_path, &ca_key_path).await?;
                }

                let (cert_pem, key_pem) =
                    generate_local_ca_with_validity_and_dn(ca_valid_secs, &ca_dn)
                        .map_err(|e| anyhow::anyhow!("failed to generate local CA: {e}"))?;
                atomic_write_file_0600(&ca_cert_path, &cert_pem).await?;
                atomic_write_file_0600(&ca_key_path, &key_pem).await?;

                let fp = sha256_fingerprint_first_cert_pem(&cert_pem)
                    .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));
                info!(
                    "Generated new local CA certificate (sha256_fingerprint={})",
                    fp
                );

                (cert_pem, key_pem)
            }
        };

        // 2) Load or create leaf signed by CA
        let cert_path = dir.join(CERT_FILE_NAME);
        let key_path = dir.join(KEY_FILE_NAME);

        let cert_exists = tokio::fs::try_exists(cert_path.clone()).await?;
        let key_exists = tokio::fs::try_exists(key_path.clone()).await?;

        if cert_exists && key_exists {
            let leaf_cert_pem = read_tls_file(&cert_path).await?;
            let leaf_key_pem = read_private_tls_file(&key_path).await?;

            match validate_leaf_signed_by_ca_cert_pem(&leaf_cert_pem, &leaf_dn, &ca_dn) {
                Ok(()) => {
                    let fp = sha256_fingerprint_first_cert_pem(&leaf_cert_pem)
                        .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));
                    if let Ok(details) = inspect_self_signed_cert_pem(&leaf_cert_pem) {
                        info!(
                            "Loaded cached leaf certificate (sha256_fingerprint={}, expires {}, remaining {})",
                            fp, details.not_after, details.remaining_human
                        );
                    } else {
                        info!("Loaded cached leaf certificate (sha256_fingerprint={})", fp);
                    }
                    let chain_pem = concat_pem_chain(&leaf_cert_pem, &ca_cert_pem);
                    return Ok(TlsMaterial {
                        chain_pem,
                        leaf_key_pem,
                        leaf_cert_pem,
                    });
                }
                Err(err) => {
                    info!("Cached leaf invalid: {err}; deleting and regenerating");
                    delete_cached_pair(&cert_path, &key_path).await?;
                }
            }
        } else if cert_exists || key_exists {
            info!(
                "Cached leaf cert/key incomplete; deleting and regenerating (cert_exists={}, key_exists={})",
                cert_exists, key_exists
            );
            delete_cached_pair(&cert_path, &key_path).await?;
        }

        let (leaf_cert_pem, leaf_key_pem) = generate_leaf_signed_by_local_ca(
            &ca_cert_pem,
            &ca_key_pem,
            sans,
            leaf_valid_secs,
            &leaf_dn,
        )?;

        let fp = sha256_fingerprint_first_cert_pem(&leaf_cert_pem)
            .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));
        if let Ok(details) = inspect_self_signed_cert_pem(&leaf_cert_pem) {
            info!(
                "Generated new leaf certificate (sha256_fingerprint={}, expires {}, remaining {})",
                fp, details.not_after, details.remaining_human
            );
        } else {
            info!("Generated new leaf certificate (sha256_fingerprint={})", fp);
        }

        // Write with secure perms atomically (no chmod race).
        atomic_write_file_0600(&cert_path, &leaf_cert_pem).await?;
        atomic_write_file_0600(&key_path, &leaf_key_pem).await?;

        let chain_pem = concat_pem_chain(&leaf_cert_pem, &ca_cert_pem);
        return Ok(TlsMaterial {
            chain_pem,
            leaf_key_pem,
            leaf_cert_pem,
        });
    }

    info!("Generating ephemeral self-signed TLS certificate; not cached");

    let (cert_pem, key_pem) = generate_self_signed_with_validity(sans.clone(), leaf_valid_secs)?;

    let fp = sha256_fingerprint_first_cert_pem(&cert_pem)
        .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));

    let details = inspect_self_signed_cert_pem(&cert_pem).ok();

    if let Some(details) = details {
        info!(
            "Generated new self-signed TLS certificate (sans={:?}, sha256_fingerprint={}, expires {}, remaining {})",
            sans, fp, details.not_after, details.remaining_human
        );
    } else {
        info!(
            "Generated new self-signed TLS certificate (sha256_fingerprint={})",
            fp
        );
    }

    // return cert with no CA:
    let chain_pem = cert_pem.clone();
    Ok(TlsMaterial {
        chain_pem,
        leaf_key_pem: key_pem,
        leaf_cert_pem: cert_pem,
    })
}

pub async fn renew_self_signed_loop(
    rustls_config: axum_server::tls_rustls::RustlsConfig,
    cache_dir: Option<std::path::PathBuf>,
    sans: Vec<String>,
    valid_secs: u32,
    renew_margin: Duration,
    mut current_cert_pem: Vec<u8>,
) {
    let min_sleep = Duration::from_secs(1);
    let validity = Duration::from_secs(valid_secs as u64);
    let renew_margin = renew_margin
        .min(validity.saturating_sub(Duration::from_secs(1)))
        .max(Duration::from_secs(1));
    let renew_every = validity
        .saturating_sub(renew_margin)
        .max(Duration::from_secs(1));
    info!(
        "Starting scheduled process to renew certificate; will renew {}s before expiry (~every {}); cache_dir={:?}; sans={:?}",
        renew_margin.as_secs(),
        format_remaining_compact(renew_every.try_into().expect("TODO remove this expect")),
        cache_dir,
        sans,
    );

    loop {
        // 1) Figure out how long until expiry.
        let sleep_for = match cert_not_after(&current_cert_pem) {
            Ok(not_after) => {
                let now = SystemTime::now();

                // If cert is already expired (clock skew, parsing bug, etc), renew immediately.
                let until_expiry = match not_after.duration_since(now) {
                    Ok(d) => d,
                    Err(_) => Duration::from_secs(0),
                };

                // Sleep until we ENTER the renew window.
                if until_expiry > renew_margin {
                    (until_expiry - renew_margin).max(min_sleep)
                } else {
                    Duration::from_secs(0)
                }
            }
            Err(err) => {
                tracing::warn!(%err, "could not parse cert expiry; will retry soon");
                Duration::from_secs(2)
            }
        };

        if !sleep_for.is_zero() {
            tokio::time::sleep(sleep_for).await;
        }

        // 2) We are in the renew window → FORCE create a fresh cert (do NOT load-or-generate).
        match generate_and_persist_self_signed(cache_dir.clone(), sans.clone(), valid_secs).await {
            Ok(tls_material) => {
                fn pem_header(pem: &[u8]) -> String {
                    let first_line = pem.split(|&b| b == b'\n').next().unwrap_or(&[]);
                    String::from_utf8_lossy(first_line).to_string()
                }

                // in renew loop, before reload:
                tracing::debug!(
                    cert_header = %pem_header(&tls_material.chain_pem),
                    key_header  = %pem_header(&tls_material.leaf_key_pem),
                    "about to reload rustls pem"
                );

                if let Err(err) = rustls_config
                    .reload_from_pem(
                        tls_material.chain_pem.clone(),
                        tls_material.leaf_key_pem.clone(),
                    )
                    .await
                {
                    tracing::error!(%err, "failed to reload rustls config; will retry");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }

                tracing::info!("reloaded self-signed certificate");
                current_cert_pem = tls_material.leaf_cert_pem;
            }
            Err(err) => {
                tracing::error!(%err, "failed to generate new self-signed cert; will retry");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn cert_not_after(cert_pem: &[u8]) -> anyhow::Result<SystemTime> {
    use std::io::Cursor;

    // rustls-pemfile gives you DER bytes for the first cert in the PEM.
    let mut reader = Cursor::new(cert_pem);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed reading PEM certs")?;

    let first = certs
        .into_iter()
        .next()
        .context("no certificate found in PEM")?;

    let (_, x509) = x509_parser::parse_x509_certificate(&first).context("failed parsing x509")?;

    // x509-parser gives an OffsetDateTime (seconds since Unix epoch).
    let ts = x509.validity().not_after.timestamp();
    let ts_u64: u64 = ts.try_into().context("cert not_after before Unix epoch")?;

    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(ts_u64))
}

async fn generate_and_persist_self_signed(
    cache_dir: Option<std::path::PathBuf>,
    sans: Vec<String>,
    valid_secs: u32,
) -> anyhow::Result<TlsMaterial> {
    let leaf_dn = default_self_signed_dn();

    // Ephemeral path: self-signed leaf only
    if cache_dir.is_none() {
        let (leaf_cert_pem, leaf_key_pem) = generate_self_signed_with_validity(sans, valid_secs)
            .map_err(|e| anyhow::anyhow!("failed to generate self-signed cert: {e}"))?;

        // In ephemeral mode, "chain" is just the leaf.
        let chain_pem = leaf_cert_pem.clone();
        return Ok(TlsMaterial {
            chain_pem,
            leaf_key_pem,
            leaf_cert_pem,
        });
    }

    let dir = cache_dir.unwrap();
    create_private_dir_all_0700(&dir).await?;
    validate_private_dir_0700(&dir).await?;

    let ca_cert_path = dir.join(CA_CERT_FILE_NAME);
    let ca_key_path = dir.join(CA_KEY_FILE_NAME);

    // CA must exist by now (created by load_or_generate_self_signed).
    let ca_cert_pem = read_tls_file(&ca_cert_path).await?;
    let ca_key_pem = read_private_tls_file(&ca_key_path).await?;

    let (leaf_cert_pem, leaf_key_pem) =
        generate_leaf_signed_by_local_ca(&ca_cert_pem, &ca_key_pem, sans, valid_secs, &leaf_dn)?;

    // Log fingerprint of the leaf (that's what clients see as "the server cert").
    let fp = sha256_fingerprint_first_cert_pem(&leaf_cert_pem)
        .unwrap_or_else(|e| format!("(fingerprint unavailable: {e})"));

    if let Ok(details) = inspect_self_signed_cert_pem(&leaf_cert_pem) {
        info!(
            "Generated replacement leaf certificate (sha256_fingerprint={}, expires {}, remaining {})",
            fp, details.not_after, details.remaining_human
        );
    } else {
        info!(
            "Generated replacement leaf certificate (sha256_fingerprint={})",
            fp
        );
    }

    // Persist leaf + leaf key (CA is stable and already persisted)
    let cert_path = dir.join(CERT_FILE_NAME);
    let key_path = dir.join(KEY_FILE_NAME);
    atomic_write_file_0600(&cert_path, &leaf_cert_pem).await?;
    atomic_write_file_0600(&key_path, &leaf_key_pem).await?;

    // Build chain for rustls: leaf first, then CA.
    let chain_pem = concat_pem_chain(&leaf_cert_pem, &ca_cert_pem);

    Ok(TlsMaterial {
        chain_pem,
        leaf_key_pem,
        leaf_cert_pem,
    })
}

fn sha256_fingerprint_first_cert_pem(pem: &[u8]) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    let mut reader = Cursor::new(pem);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed reading PEM certs")?;

    let first = certs
        .into_iter()
        .next()
        .context("PEM contained no certificates")?;

    let digest = Sha256::digest(first.as_ref());

    // OpenSSL-ish formatting: AA:BB:CC...
    let mut out = String::new();
    for (i, b) in digest.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        use std::fmt::Write;
        write!(&mut out, "{:02X}", b).expect("write to String cannot fail");
    }

    Ok(out)
}

fn generate_local_ca_with_validity_and_dn(
    valid_secs: u32,
    dn: &SelfSignedDn,
) -> Result<(Vec<u8>, Vec<u8>), rcgen::Error> {
    let mut params = CertificateParams::default();

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(rcgen::DnType::OrganizationName, dn.organization.clone());
    distinguished_name.push(rcgen::DnType::CommonName, dn.common_name.clone());
    params.distinguished_name = distinguished_name;

    // Mark as CA
    params.is_ca = rcgen::IsCa::Ca(BasicConstraints::Unconstrained);

    // Typical CA usages
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + std::time::Duration::from_secs(valid_secs as u64);

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    Ok((
        cert.pem().into_bytes(),
        key_pair.serialize_pem().into_bytes(),
    ))
}

fn generate_leaf_signed_by_local_ca(
    ca_cert_pem: &[u8],
    ca_key_pem: &[u8],
    sans: Vec<String>,
    leaf_valid_secs: u32,
    leaf_dn: &SelfSignedDn,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    // Build an rcgen Issuer from the persisted CA cert+key.
    let ca_cert_str = std::str::from_utf8(ca_cert_pem).context("CA cert pem not utf-8")?;
    let ca_key_str = std::str::from_utf8(ca_key_pem).context("CA key pem not utf-8")?;

    let ca_key = KeyPair::from_pem(ca_key_str).context("failed to parse CA key")?;
    let issuer: Issuer<'static, KeyPair> =
        Issuer::from_ca_cert_pem(ca_cert_str, ca_key).context("failed to load CA issuer")?;

    // Generate leaf key + params
    let leaf_key = KeyPair::generate().context("failed generating leaf key")?;
    let mut params = CertificateParams::new(sans).context("failed building leaf params")?;

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(
        rcgen::DnType::OrganizationName,
        leaf_dn.organization.clone(),
    );
    distinguished_name.push(rcgen::DnType::CommonName, leaf_dn.common_name.clone());
    params.distinguished_name = distinguished_name;

    // Leaf should get AKI pointing at the CA for nicer chain validation.
    params.use_authority_key_identifier_extension = true;

    // Typical server leaf usages
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + std::time::Duration::from_secs(leaf_valid_secs as u64);

    // Sign leaf by issuer
    let cert = params
        .signed_by(&leaf_key, &issuer)
        .context("failed signing leaf by CA")?;

    Ok((
        cert.pem().into_bytes(),
        leaf_key.serialize_pem().into_bytes(),
    ))
}
