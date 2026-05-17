use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const MAX_AGE: Duration = Duration::from_secs(15);

fn state_dir() -> PathBuf {
    std::env::var("NIFTY_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/nifty-state"))
}

/// Read a state file, returning None if it doesn't exist or is stale (>15s old).
pub async fn read_state_file(filename: &str) -> Option<String> {
    let path = state_dir().join(filename);
    check_freshness(&path).await?;
    tokio::fs::read_to_string(&path).await.ok()
}

/// Check if a state file exists and is fresh (modified within MAX_AGE).
pub async fn is_state_fresh(filename: &str) -> bool {
    let path = state_dir().join(filename);
    check_freshness(&path).await.is_some()
}

/// Check if a file exists and was modified within MAX_AGE.
async fn check_freshness(path: &Path) -> Option<()> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).unwrap_or(Duration::MAX);
    if age > MAX_AGE {
        None
    } else {
        Some(())
    }
}
