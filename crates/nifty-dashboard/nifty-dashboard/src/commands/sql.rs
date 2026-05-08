use std::io::Write;

use crate::{
    config::{ServeConfig, database::build_db_url},
    errors::CliError,
};

use std::process::{Command, Stdio};

fn sqlite_path_from_db_url(db_url: &str) -> Result<String, CliError> {
    let s = db_url.trim();

    // Accept plain paths (relative or absolute)
    if !s.starts_with("sqlite:") {
        return Ok(s.to_string());
    }

    // Strip scheme
    let mut rest = &s["sqlite:".len()..];

    // Common forms:
    // - sqlite:/abs/path.db
    // - sqlite:///abs/path.db
    // - sqlite://relative/path.db   (less common, but seen)
    // - sqlite:relative/path.db
    //
    // For sqlite, URLs often use 3 slashes for absolute paths.
    // We'll normalize by removing a leading `//` (authority) if present,
    // then ensuring absolute paths keep their leading `/`.
    if let Some(stripped) = rest.strip_prefix("///") {
        rest = stripped; // absolute path without the extra URL slashes
        return Ok(format!("/{}", rest.trim_start_matches('/')));
    }

    if let Some(stripped) = rest.strip_prefix("//") {
        // sqlite://... (treat as path-ish; drop the leading //)
        rest = stripped;
    }

    // At this point rest may start with / (absolute) or not (relative).
    Ok(rest.to_string())
}

pub fn sql<W1: Write, W2: Write>(
    args: ServeConfig,
    root_dir: std::path::PathBuf,
    _out: &mut W1,
    _err: &mut W2,
) -> Result<(), CliError> {
    let db_url = build_db_url(args.app.database.url, &root_dir);
    let db_path = sqlite_path_from_db_url(&db_url)?;
    let command = args.app.database.sqlite_path;

    // Optional: log what we're about to open
    println!();
    println!("## Running {command}");
    println!("## Opening database {}", db_path);

    let status = Command::new(&command)
        .arg(db_path)
        // interactive shell needs inherited stdio
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            // Adjust mapping to your CliError variants
            CliError::InvalidArgs(format!(
                "Failed to launch sqlite3 (is it installed and on PATH?): {e}"
            ))
        })?;

    if !status.success() {
        return Err(CliError::InvalidArgs(format!(
            "{command} exited with status: {status}"
        )));
    }

    Ok(())
}
