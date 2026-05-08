use super::{cert_details::CertDetails, generate::SelfSignedDn};
use ::time::OffsetDateTime;
use anyhow::{Context, bail};
use rustls_pemfile::certs as load_pem_certs;
use std::path::Path;
use tokio::fs;
use x509_parser::prelude::*;

pub fn inspect_self_signed_cert_pem(cert_pem: &[u8]) -> anyhow::Result<CertDetails> {
    let der = extract_single_cert_der(cert_pem)?;
    let (_rem, x509) =
        parse_x509_certificate(&der).map_err(|e| anyhow::anyhow!("x509 parse error: {e}"))?;

    let validity = x509.validity();

    // In your x509-parser version, this returns OffsetDateTime directly.
    let not_before = validity.not_before.to_datetime();
    let not_after = validity.not_after.to_datetime();

    let now = OffsetDateTime::now_utc();

    Ok(CertDetails::new(
        not_before,
        not_after,
        now,
        x509.subject().to_string(),
        x509.issuer().to_string(),
    ))
}

/// Validate cert:
/// Check expiration and expected issuer and common names.
#[allow(dead_code)]
pub fn validate_self_signed_cert_pem(
    cert_pem: &[u8],
    expected: &SelfSignedDn,
) -> anyhow::Result<()> {
    let der = extract_single_cert_der(cert_pem)?;
    let (_rem, x509) =
        parse_x509_certificate(&der).map_err(|e| anyhow::anyhow!("x509 parse error: {e}"))?;

    validate_validity_now(&x509)?;
    validate_subject_dn_exact(&x509, &expected.organization, &expected.common_name)?;
    validate_issuer_matches_subject(&x509)?;
    Ok(())
}

fn extract_single_cert_der(cert_pem: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut slice: &[u8] = cert_pem;
    let mut iter = load_pem_certs(&mut slice);

    let first = iter
        .next()
        .transpose()
        .context("failed to decode PEM certificate")?
        .context("PEM contained no certificates")?;

    // Enforce “exactly one cert” for the cache file. If you prefer to allow chains,
    // loosen this.
    if iter.next().is_some() {
        bail!("cached PEM contained multiple certificates; expected exactly one");
    }

    Ok(first.as_ref().to_vec())
}

fn validate_validity_now(x509: &X509Certificate<'_>) -> anyhow::Result<()> {
    let validity = x509.validity();
    let now = ASN1Time::now();

    if !validity.is_valid_at(now) {
        bail!("certificate is expired or not yet valid");
    }
    Ok(())
}
#[allow(dead_code)]
fn validate_subject_dn_exact(
    x509: &X509Certificate<'_>,
    expected_org: &str,
    expected_cn: &str,
) -> anyhow::Result<()> {
    let subject = x509.subject();

    if subject.iter_attributes().count() != 2 {
        bail!("certificate subject DN must contain exactly O and CN (no extra attributes)");
    }

    fn attr_to_string(attr: &AttributeTypeAndValue<'_>) -> anyhow::Result<String> {
        Ok(attr
            .as_str()
            .context("certificate DN contains non-UTF8 or unsupported ASN.1 string")?
            .to_string())
    }

    let mut org_it = subject.iter_organization();
    let org_attr = org_it
        .next()
        .context("certificate subject missing OrganizationName")?;
    if org_it.next().is_some() {
        bail!("certificate subject contains multiple OrganizationName attributes");
    }

    let mut cn_it = subject.iter_common_name();
    let cn_attr = cn_it
        .next()
        .context("certificate subject missing CommonName")?;
    if cn_it.next().is_some() {
        bail!("certificate subject contains multiple CommonName attributes");
    }

    let org = attr_to_string(org_attr)?;
    let cn = attr_to_string(cn_attr)?;

    if org != expected_org || cn != expected_cn {
        bail!(
            "certificate subject DN mismatch (expected O='{}', CN='{}')",
            expected_org,
            expected_cn
        );
    }

    Ok(())
}

fn validate_issuer_matches_subject(x509: &X509Certificate<'_>) -> anyhow::Result<()> {
    // We want self-signed in the cache (issuer == subject).
    // Comparing stringified forms is good enough here because we also validated the exact subject.
    if x509.issuer() != x509.subject() {
        bail!("certificate issuer does not match subject (not self-signed)");
    }
    Ok(())
}

/// Read a TLS file (certificate or other non-secret).
///
/// Unix policy:
/// - must be a regular file (not a symlink)
/// - must be readable by this process
/// - DOES NOT enforce mode bits (certs are often 0644)
pub async fn read_tls_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use anyhow::{Context, bail};
        use std::io;
        use tokio::fs;

        let meta = fs::symlink_metadata(path)
            .await
            .with_context(|| format!("failed to stat '{}'", path.display()))?;

        if meta.file_type().is_symlink() {
            bail!("refusing to use symlink for TLS file '{}'", path.display());
        }
        if !meta.file_type().is_file() {
            bail!("TLS path '{}' is not a regular file", path.display());
        }

        match fs::read(path).await {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                bail!(
                    "cannot read TLS file '{}' (permission denied). Fix permissions/ownership.",
                    path.display()
                );
            }
            Err(e) => Err(anyhow::anyhow!(
                "failed to read TLS file '{}': {e}",
                path.display()
            )),
        }
    }

    #[cfg(not(unix))]
    {
        Ok(tokio::fs::read(path).await?)
    }
}

/// Enforce TLS cache file security policy (Unix):
/// - must be a regular file (not a symlink)
/// - no group/other permission bits
/// - must be readable by *this process* (otherwise error)
pub async fn read_private_tls_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use anyhow::{Context, bail};
        use std::io;
        use std::os::unix::fs::PermissionsExt;
        use tokio::fs;

        let meta = fs::symlink_metadata(path)
            .await
            .with_context(|| format!("failed to stat '{}'", path.display()))?;

        if meta.file_type().is_symlink() {
            bail!(
                "refusing to use symlink for TLS cache file '{}'",
                path.display()
            );
        }
        if !meta.file_type().is_file() {
            bail!("TLS cache path '{}' is not a regular file", path.display());
        }

        let mode = meta.permissions().mode() & 0o777;

        // Disallow any group/other permissions.
        if (mode & 0o077) != 0 {
            bail!(
                "insecure permissions on '{}': mode {:o}; expected no group/other permissions (e.g. chmod 600)",
                path.display(),
                mode
            );
        }
        if (mode & 0o100) != 0 {
            bail!(
                "TLS cache file '{}' should not be executable (mode {:o}); refusing",
                path.display(),
                mode
            );
        }

        // Now prove we can read it. If we can't, that's a hard error.
        match fs::read(path).await {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                bail!(
                    "cannot read TLS cache file '{}' (permission denied). \
                     Fix ownership/permissions (e.g. chmod 600, chown to this user).",
                    path.display()
                );
            }
            Err(e) => Err(anyhow::anyhow!(
                "failed to read TLS cache file '{}': {e}",
                path.display()
            )),
        }
    }

    #[cfg(not(unix))]
    {
        // Non-Unix: just read it (or add platform-specific ACL checks later).
        Ok(tokio::fs::read(path).await?)
    }
}

/// If validation fails, you wanted us to delete the cached files.
/// This helper tries to delete both (best-effort), but returns an error if deletion fails
/// for reasons other than NotFound.
pub async fn delete_cached_pair(cert_path: &Path, key_path: &Path) -> anyhow::Result<()> {
    delete_one(cert_path).await?;
    delete_one(key_path).await?;
    Ok(())
}

async fn delete_one(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "failed to delete '{}': {e}",
            path.display()
        )),
    }
}

fn validate_name_dn_exact(
    name: &X509Name<'_>,
    expected_org: &str,
    expected_cn: &str,
) -> anyhow::Result<()> {
    if name.iter_attributes().count() != 2 {
        bail!("DN must contain exactly O and CN (no extra attributes)");
    }

    fn attr_to_string(attr: &AttributeTypeAndValue<'_>) -> anyhow::Result<String> {
        Ok(attr
            .as_str()
            .context("certificate DN contains non-UTF8 or unsupported ASN.1 string")?
            .to_string())
    }

    let mut org_it = name.iter_organization();
    let org_attr = org_it.next().context("DN missing OrganizationName")?;
    if org_it.next().is_some() {
        bail!("DN contains multiple OrganizationName attributes");
    }

    let mut cn_it = name.iter_common_name();
    let cn_attr = cn_it.next().context("DN missing CommonName")?;
    if cn_it.next().is_some() {
        bail!("DN contains multiple CommonName attributes");
    }

    let org = attr_to_string(org_attr)?;
    let cn = attr_to_string(cn_attr)?;

    if org != expected_org || cn != expected_cn {
        bail!(
            "DN mismatch (expected O='{}', CN='{}')",
            expected_org,
            expected_cn
        );
    }

    Ok(())
}

fn validate_is_ca(x509: &X509Certificate<'_>) -> anyhow::Result<()> {
    // basicConstraints must exist and indicate CA=true
    let bc = x509
        .basic_constraints()
        .context("missing basicConstraints extension")?
        .context("basicConstraints not present")?;

    if !bc.value.ca {
        bail!("certificate basicConstraints CA flag is false");
    }
    Ok(())
}

pub fn validate_local_ca_cert_pem(cert_pem: &[u8], expected: &SelfSignedDn) -> anyhow::Result<()> {
    let der = extract_single_cert_der(cert_pem)?;
    let (_rem, x509) =
        parse_x509_certificate(&der).map_err(|e| anyhow::anyhow!("x509 parse error: {e}"))?;

    validate_validity_now(&x509)?;
    validate_name_dn_exact(
        x509.subject(),
        &expected.organization,
        &expected.common_name,
    )?;
    validate_issuer_matches_subject(&x509)?;
    validate_is_ca(&x509)?;
    Ok(())
}

pub fn validate_leaf_signed_by_ca_cert_pem(
    cert_pem: &[u8],
    expected_leaf: &SelfSignedDn,
    expected_issuer: &SelfSignedDn,
) -> anyhow::Result<()> {
    let der = extract_single_cert_der(cert_pem)?;
    let (_rem, x509) =
        parse_x509_certificate(&der).map_err(|e| anyhow::anyhow!("x509 parse error: {e}"))?;

    validate_validity_now(&x509)?;
    validate_name_dn_exact(
        x509.subject(),
        &expected_leaf.organization,
        &expected_leaf.common_name,
    )?;
    validate_name_dn_exact(
        x509.issuer(),
        &expected_issuer.organization,
        &expected_issuer.common_name,
    )?;

    // Leaf should not be self-signed.
    if x509.issuer() == x509.subject() {
        bail!("leaf certificate is unexpectedly self-signed");
    }

    Ok(())
}
