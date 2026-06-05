use std::path::Path;
use tokio::fs;

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

/// Delete a pair of cached files (best-effort, ignores NotFound).
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
