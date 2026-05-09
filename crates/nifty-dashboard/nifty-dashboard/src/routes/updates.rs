use aide::axum::ApiRouter;
use api_doc_macros::{api_doc, get_with_docs};
use axum::Json;
use axum::extract::State;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    errors::ErrorBody,
    response::{ApiJson, ApiResponse, json_ok},
    AppState,
};

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(get_updates))
}

// --- Response types ---

#[derive(Serialize, JsonSchema)]
struct UpdatesResponse {
    nixos_version: Option<String>,
    nixpkgs_date: Option<String>,
    nixpkgs_age_seconds: Option<u64>,
    built_at: Option<String>,
    built_age_seconds: Option<u64>,
    nifty_filter_version: Option<String>,
    kernel_version: Option<String>,
}

// --- Handler ---

#[api_doc(
    id = "get_updates",
    tag = "updates",
    ok = "Json<ApiResponse<UpdatesResponse>>",
    err = "Json<ErrorBody>"
)]
/// System version info
///
/// Returns NixOS version, build date, and nifty-filter version.
async fn get_updates(_state: State<AppState>) -> ApiJson<UpdatesResponse> {
    let (nixos_version, kernel_version, built_at_info) =
        tokio::join!(read_nixos_version(), read_kernel_version(), read_built_at());

    let (nixpkgs_date, nixpkgs_age_seconds) = nixos_version
        .as_deref()
        .and_then(|v| parse_nixpkgs_date(v))
        .unzip();

    let (built_at, built_age_seconds) = built_at_info.unzip();

    let nifty_filter_version = option_env!("GIT_SHA")
        .filter(|s| !s.is_empty())
        .map(|sha| format!("{} ({})", env!("CARGO_PKG_VERSION"), sha));

    json_ok(UpdatesResponse {
        nixos_version,
        nixpkgs_date,
        nixpkgs_age_seconds,
        built_at,
        built_age_seconds,
        nifty_filter_version,
        kernel_version,
    })
}

/// Parse nixpkgs commit date from NixOS version string like "25.05.20260414.4bd9165"
/// Returns (ISO date string, age in seconds)
fn parse_nixpkgs_date(version: &str) -> Option<(String, u64)> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let date_str = parts[2];
    if date_str.len() != 8 {
        return None;
    }
    let year: i32 = date_str[0..4].parse().ok()?;
    let month: u32 = date_str[4..6].parse().ok()?;
    let day: u32 = date_str[6..8].parse().ok()?;
    if month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    let iso_date = format!("{:04}-{:02}-{:02}", year, month, day);

    let build_ts = date_to_epoch(year, month, day)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let age = now.saturating_sub(build_ts);

    Some((iso_date, age))
}

/// Convert a date to unix epoch (midnight UTC)
fn date_to_epoch(year: i32, month: u32, day: u32) -> Option<u64> {
    let mut y = year as i64;
    let mut m = month as i64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m - 3) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400) as u64)
}

/// Read the actual build timestamp from the system closure's modification time.
async fn read_built_at() -> Option<(String, u64)> {
    // /run/current-system is a symlink to the active NixOS system closure.
    // Its target's mtime in the Nix store reflects when it was built.
    let metadata = tokio::fs::symlink_metadata("/run/current-system").await.ok()?;
    let modified = metadata.modified().ok()?;
    let build_ts = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let age = now.saturating_sub(build_ts);

    // Format as ISO 8601 date-time (UTC)
    let secs_in_day = 86400u64;
    let days = build_ts / secs_in_day;
    let time_of_day = build_ts % secs_in_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    // Convert days since epoch to y-m-d
    let (year, month, day) = epoch_days_to_date(days as i64);
    let iso = format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", year, month, day, hours, minutes);

    Some((iso, age))
}

/// Convert days since unix epoch to (year, month, day)
fn epoch_days_to_date(days: i64) -> (i32, u32, u32) {
    // Civil calendar from day count (Algorithm from Howard Hinnant)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

async fn read_nixos_version() -> Option<String> {
    // /run/current-system/nixos-version has the full version like "25.05.20260414.4bd9165"
    if let Ok(contents) = tokio::fs::read_to_string("/run/current-system/nixos-version").await {
        let version = contents.trim();
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }
    // Fallback: /etc/os-release VERSION_ID (shorter, e.g. "25.05")
    let contents = tokio::fs::read_to_string("/etc/os-release").await.ok()?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("VERSION_ID=") {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

async fn read_kernel_version() -> Option<String> {
    // /proc/sys/kernel/osrelease is simpler and cleaner than parsing /proc/version
    let contents = tokio::fs::read_to_string("/proc/sys/kernel/osrelease")
        .await
        .ok()?;
    let version = contents.trim();
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}
