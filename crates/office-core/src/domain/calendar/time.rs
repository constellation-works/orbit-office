use super::CalendarError;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone};
use chrono_tz::Tz;

/// Resolve IANA zones or CLDR's global Windows-to-IANA mapping.
pub fn zone(name: &str) -> Result<Tz, CalendarError> {
    if let Ok(zone) = name.parse() {
        return Ok(zone);
    }
    include_str!("windows-zones.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(windows, _)| *windows == name)
        .and_then(|(_, iana)| iana.parse().ok())
        .ok_or(CalendarError::Zone)
}
/// Strict RFC 3339 parsing.
pub fn parse_time(text: &str) -> Result<DateTime<FixedOffset>, CalendarError> {
    DateTime::parse_from_rfc3339(text).map_err(|_| CalendarError::Invalid)
}
/// Canonical UTC key/time.
pub fn stamp(time: DateTime<FixedOffset>) -> String {
    time.with_timezone(&chrono::Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}
pub(super) fn local(text: &str, tz: &str) -> Result<(DateTime<FixedOffset>, bool), CalendarError> {
    let (naive, all_day) = if text.len() == 8 {
        (
            NaiveDate::parse_from_str(text, "%Y%m%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .ok_or(CalendarError::Invalid)?,
            true,
        )
    } else {
        (
            NaiveDateTime::parse_from_str(text.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
                .map_err(|_| CalendarError::Invalid)?,
            false,
        )
    };
    // Floating times have no unambiguous instant; do not interpret them as UTC.
    let tz = zone(if text.ends_with('Z') || all_day {
        "UTC"
    } else {
        tz
    })?;
    let dt = tz
        .from_local_datetime(&naive)
        .earliest()
        .ok_or(CalendarError::Zone)?;
    Ok((dt.fixed_offset(), all_day))
}
pub(super) fn at(naive: NaiveDateTime, tz: &str) -> Result<DateTime<FixedOffset>, CalendarError> {
    zone(tz)?
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.fixed_offset())
        .ok_or(CalendarError::Zone)
}
