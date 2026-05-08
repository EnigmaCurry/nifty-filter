use serde::Serialize;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Serialize)]
pub struct CertDetails {
    // If you want stable JSON, use explicit RFC3339:
    #[serde(with = "time::serde::rfc3339")]
    pub not_before: OffsetDateTime,

    #[serde(with = "time::serde::rfc3339")]
    pub not_after: OffsetDateTime,

    /// not_after - now (negative if already expired)
    pub remaining_seconds: i64,

    /// Human-friendly presentation (e.g. "12d 3h 5m")
    pub remaining_human: String,

    pub subject: String,
    pub issuer: String,
}

impl CertDetails {
    pub fn new(
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
        now: OffsetDateTime,
        subject: String,
        issuer: String,
    ) -> Self {
        let remaining = not_after - now;
        Self {
            not_before,
            not_after,
            remaining_seconds: remaining.whole_seconds(),
            remaining_human: format_remaining_compact(remaining),
            subject,
            issuer,
        }
    }
}

pub fn format_remaining_compact(d: Duration) -> String {
    let secs = d.whole_seconds();
    let sign = if secs < 0 { "-" } else { "" };
    let secs_u = secs.unsigned_abs(); // u64

    let days = secs_u / 86_400;
    let hours = (secs_u % 86_400) / 3_600;
    let mins = (secs_u % 3_600) / 60;
    let secs = secs_u % 60;

    if days > 0 {
        format!("{sign}{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{sign}{hours}h {mins}m {secs}s")
    } else if mins > 0 {
        format!("{sign}{mins}m {secs}s")
    } else {
        format!("{sign}{secs}s")
    }
}
