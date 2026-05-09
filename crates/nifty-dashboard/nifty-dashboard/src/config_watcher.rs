use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, trace, warn};

const DEBOUNCE: Duration = Duration::from_millis(500);

pub fn config_file_path() -> PathBuf {
    std::env::var("NIFTY_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/nifty-filter/nifty-filter.hcl"))
}

pub async fn read_boot_sha() -> String {
    // Read the boot-time SHA from the file written by nifty-config-sha.service.
    // Falls back to computing it from the config file if the boot SHA file doesn't exist
    // (e.g. running outside NixOS).
    let boot_sha_path = std::env::var("NIFTY_CONFIG_BOOT_SHA_FILE")
        .unwrap_or_else(|_| "/run/nifty-filter/config-boot-sha".to_string());
    if let Ok(contents) = tokio::fs::read_to_string(&boot_sha_path).await {
        let sha = contents.trim().to_string();
        if !sha.is_empty() {
            return sha;
        }
    }
    // Fallback: compute from current config file
    compute_config_sha().await
}

pub async fn compute_config_sha() -> String {
    let path = config_file_path();
    match tokio::fs::read(&path).await {
        Ok(contents) => format!("{:x}", Sha256::digest(&contents)),
        Err(_) => String::new(),
    }
}

pub fn spawn_config_watcher(tx: broadcast::Sender<()>) {
    let path = config_file_path();
    info!("watching config file for changes: {}", path.display());

    // Watch the parent directory (handles atomic writes that replace the file)
    let watch_dir = path.parent().unwrap_or(&path).to_path_buf();
    let file_name = path.file_name().map(|n| n.to_os_string());

    std::thread::spawn(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();

        let mut watcher: RecommendedWatcher =
            match Watcher::new(notify_tx, notify::Config::default()) {
                Ok(w) => w,
                Err(e) => {
                    warn!("failed to create file watcher: {e}");
                    return;
                }
            };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            warn!("failed to watch {}: {e}", watch_dir.display());
            return;
        }

        debug!("file watcher active on {}", watch_dir.display());

        let mut last_notify = Instant::now() - DEBOUNCE;

        for event in notify_rx {
            match event {
                Ok(event) => {
                    trace!("fs event: {:?}", event);

                    // Match writes, creates, and renames (atomic save via rename)
                    let dominated = matches!(
                        event.kind,
                        EventKind::Modify(ModifyKind::Data(_))
                            | EventKind::Modify(ModifyKind::Name(_))
                            | EventKind::Create(_)
                    );
                    if !dominated {
                        continue;
                    }

                    // Filter to our specific file
                    let matches = match &file_name {
                        Some(name) => event.paths.iter().any(|p| {
                            p.file_name().map_or(false, |n| n == name.as_os_str())
                        }),
                        None => true,
                    };

                    if matches {
                        let now = Instant::now();
                        if now.duration_since(last_notify) < DEBOUNCE {
                            trace!("debounced duplicate config change event");
                            continue;
                        }
                        last_notify = now;
                        info!("config file changed ({:?}), notifying clients", event.kind);
                        let _ = tx.send(());
                    }
                }
                Err(e) => {
                    warn!("file watcher error: {e}");
                }
            }
        }
    });
}
