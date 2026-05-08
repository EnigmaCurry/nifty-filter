use anyhow::Context;
use std::path::Path;

pub async fn validate_private_dir_0700(dir: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use anyhow::bail;
        use std::os::unix::fs::PermissionsExt;

        let meta = tokio::fs::symlink_metadata(dir)
            .await
            .with_context(|| format!("failed to stat dir '{}'", dir.display()))?;

        if meta.file_type().is_symlink() {
            bail!(
                "refusing to use symlink as TLS cache dir '{}'",
                dir.display()
            );
        }
        if !meta.file_type().is_dir() {
            bail!("TLS cache dir '{}' is not a directory", dir.display());
        }

        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            bail!(
                "insecure permissions on TLS cache dir '{}': mode {:o}; expected 700 (chmod 700 '{}')",
                dir.display(),
                mode,
                dir.display()
            );
        }
    }

    #[cfg(not(unix))]
    {
        let _ = dir;
    }

    Ok(())
}

/// Async: create a private directory (recursively) with mode 0700 on Unix, then validate.
/// Safe to call from async contexts (server startup, etc).
pub async fn create_private_dir_all_0700(dir: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let dir_display = dir.display().to_string();
        let dir_owned = dir.to_owned();

        tokio::task::spawn_blocking(move || {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700) // requested (still subject to umask)
                .create(&dir_owned)
        })
        .await
        .context("failed to join blocking task for create_private_dir_all_0700")?
        .with_context(|| format!("failed to create directory '{}'", dir_display))?;

        // Force final perms regardless of umask.
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .await
            .with_context(|| format!("failed to chmod 700 '{}'", dir.display()))?;
    }

    #[cfg(not(unix))]
    {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("failed to create directory '{}'", dir.display()))?;
    }

    validate_private_dir_0700(dir).await?;
    Ok(())
}

/// Sync wrapper for non-async callers (config/CLI).
///
/// This does **not** require a Tokio runtime. It uses std::fs on Unix/other.
/// It enforces the same policy, but does validation synchronously.
pub fn create_private_dir_all_0700_sync(dir: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use anyhow::bail;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("failed to create directory '{}'", dir.display()))?;

        // Force final perms regardless of umask.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to chmod 700 '{}'", dir.display()))?;

        let meta = std::fs::symlink_metadata(dir)
            .with_context(|| format!("failed to stat dir '{}'", dir.display()))?;

        if meta.file_type().is_symlink() {
            bail!("refusing to load data from a symlink '{}'", dir.display());
        }
        if !meta.file_type().is_dir() {
            bail!("path '{}' is not a directory", dir.display());
        }

        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            bail!(
                "insecure permissions on directory '{}': mode {:o}; expected 700 (chmod 700 '{}')",
                dir.display(),
                mode,
                dir.display()
            );
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create directory '{}'", dir.display()))?;
        Ok(())
    }
}

pub async fn atomic_write_file_0600(path: &std::path::Path, contents: &[u8]) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    use tokio::io::AsyncWriteExt;

    #[cfg(unix)]
    {
        let parent = path.parent().context("path must have a parent directory")?;

        // Temp file in same directory so rename is atomic.
        let mut tmp = parent.join(format!(
            ".{}.tmp",
            path.file_name().and_then(|s| s.to_str()).unwrap_or("tls")
        ));

        // If the tmp name collides, add some entropy.
        if tmp.exists() {
            tmp = parent.join(format!(
                ".{}.tmp.{}",
                path.file_name().and_then(|s| s.to_str()).unwrap_or("tls"),
                std::process::id()
            ));
        }

        // Create tmp with 0600 from the start.
        let mut f = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)
            .await
            .with_context(|| format!("failed to create temp file '{}'", tmp.display()))?;

        f.write_all(contents).await?;
        f.flush().await?;
        f.sync_all().await?;

        // Refuse to overwrite a symlink at destination (paranoia).
        if let Ok(meta) = tokio::fs::symlink_metadata(path).await
            && meta.file_type().is_symlink()
        {
            // Best-effort cleanup.
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("refusing to overwrite symlink '{}'", path.display());
        }

        // Atomic replacement.
        tokio::fs::rename(&tmp, path).await.with_context(|| {
            format!(
                "failed to rename '{}' -> '{}'",
                tmp.display(),
                path.display()
            )
        })?;

        Ok(())
    }

    #[cfg(not(unix))]
    {
        // Best effort on non-Unix.
        tokio::fs::write(path, contents)
            .await
            .with_context(|| format!("failed to write '{}'", path.display()))?;
        Ok(())
    }
}
