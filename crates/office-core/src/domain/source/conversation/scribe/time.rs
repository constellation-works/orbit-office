//! Timestamp parsing and entry-line time formatting.

use chrono::{DateTime, FixedOffset, NaiveDate};

use super::refusal;
use crate::Result;

/// Parse RFC 3339 and convert to the host zone (UTC when unknown).
pub fn local(at: &str, zone: Option<&str>) -> Result<DateTime<FixedOffset>> {
    let instant = DateTime::parse_from_rfc3339(at)
        .map_err(|_| refusal("scribe_parse_invalid", "message time is not RFC 3339"))?;
    Ok(
        match zone.and_then(|name| name.parse::<chrono_tz::Tz>().ok()) {
            Some(tz) => instant.with_timezone(&tz).fixed_offset(),
            None => instant.with_timezone(&chrono::Utc).fixed_offset(),
        },
    )
}

/// Minute-precision local time, as written on entry lines.
#[must_use]
pub fn line_time(at: DateTime<FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M%:z").to_string()
}

pub(super) fn valid_due(due: &str) -> bool {
    DateTime::parse_from_rfc3339(due).is_ok()
        || DateTime::parse_from_str(due, "%Y-%m-%dT%H:%M%:z").is_ok()
        || NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok()
}
