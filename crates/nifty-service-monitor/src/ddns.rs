use std::path::Path;

use log::{info, error};

use crate::config::{DdnsConfig, DdnsUpdaterConfig, DdnsUpdaterEntry};

/// Write the ddns-updater config.json to the given path.
///
/// The file is only written if the content has changed, to avoid
/// unnecessary container restarts via the systemd path watcher.
/// Returns true on success (or no-op), false on write error.
pub fn write_config(path: &Path, config: &DdnsConfig) -> bool {
    let entries: Vec<DdnsUpdaterEntry> = config
        .record
        .iter()
        .map(|(domain, record)| DdnsUpdaterEntry {
            provider: record.provider.clone(),
            domain: domain.clone(),
            extra: record.extra.clone(),
        })
        .collect();

    let updater_config = DdnsUpdaterConfig { settings: entries };

    let content = match serde_json::to_string_pretty(&updater_config) {
        Ok(c) => c,
        Err(e) => {
            error!("ddns: failed to serialize config: {e}");
            return false;
        }
    };

    // Only write if content changed to avoid unnecessary restarts.
    let needs_write = match std::fs::read_to_string(path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if needs_write {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("ddns: failed to create config directory {}: {e}", parent.display());
                return false;
            }
        }
        if let Err(e) = std::fs::write(path, &content) {
            error!("ddns: failed to write config to {}: {e}", path.display());
            return false;
        }
        info!(
            "ddns: wrote config with {} record(s) to {}",
            config.record.len(),
            path.display()
        );
    }

    true
}
