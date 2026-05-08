use std::path::Path;

use conf::Conf;
use serde::{Deserialize, Serialize};

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct DatabaseConfig {
    /// Database URL for sqlx (or set DATABASE_URL).
    #[arg(long = "database-url", env = "DATABASE_URL")]
    pub url: Option<String>,

    /// Path to sqlite3 binary
    #[arg(long = "sql-sqlite-path", env = "SQLITE_PATH")]
    #[conf(default("sqlite3".to_string()))]
    pub sqlite_path: String,
}

pub(crate) fn build_db_url(maybe_url: Option<String>, root_dir: &Path) -> String {
    match maybe_url.as_ref() {
        None => {
            let db_path = root_dir.join("data.db");
            format!("sqlite://{}", db_path.display())
        }
        Some(url) => url.clone(),
    }
}
