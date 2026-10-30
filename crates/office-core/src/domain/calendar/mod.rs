//! Pure calendar policy, normalization and bounded recurrence expansion.
mod graph;
mod ics;
mod local;
pub use local::{CalendarSources, LocalCalendar, LocalChange, LocalEvent};
mod time;

pub use graph::{graph_events, next_request};
pub use ics::parse_ics;
pub use time::{parse_time, stamp, zone};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Safe calendar failures. Untrusted bodies, URLs and secrets never enter diagnostics.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CalendarError {
    /// Missing calendar connection.
    #[error("choose a calendar gateway connection with office.config-set, or supply ics_file")]
    Unconfigured,
    /// Malformed or incomplete untrusted data.
    #[error("calendar data is malformed or incomplete")]
    Invalid,
    /// A timezone not in IANA or the CLDR Windows mapping.
    #[error("calendar event uses an unknown or unresolved timezone")]
    Zone,
    /// Resource or paging bound reached.
    #[error("calendar exceeds the byte, recurrence, event or ten-page limit")]
    TooLarge,
    /// Gateway refused or failed a read.
    #[error("calendar gateway read failed; check the connection and try syncing again")]
    Fetch,
    /// A Graph next link leaves the connection base.
    #[error("calendar next link is outside the connection base_url")]
    NextLink,
    /// Storage unavailable or unsafe.
    #[error("calendar storage is unavailable or unsafe in this call context")]
    Storage,
}
impl CalendarError {
    /// Stable exec-envelope code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unconfigured => "calendar_unconfigured",
            Self::Invalid => "calendar_invalid",
            Self::Zone => "calendar_unknown_zone",
            Self::TooLarge => "calendar_too_large",
            Self::Fetch => "calendar_fetch_failed",
            Self::NextLink => "calendar_next_link_refused",
            Self::Storage => "calendar_storage_unavailable",
        }
    }
}
/// Validated calendar configuration, with defaults usable by offline ICS imports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CalendarSettings {
    /// Gateway connection name, absent for offline imports.
    pub connection: Option<String>,
    /// Past window in days, 1–90.
    pub window_days_past: u32,
    /// Future window in days, 1–90.
    pub window_days_ahead: u32,
    /// Prep lead time, 15–1440 minutes.
    pub prep_lead_minutes: u32,
    /// Prep batch bound, 1–20.
    pub prep_max_per_run: u32,
}
impl Default for CalendarSettings {
    fn default() -> Self {
        Self {
            connection: None,
            window_days_past: 14,
            window_days_ahead: 30,
            prep_lead_minutes: 120,
            prep_max_per_run: 5,
        }
    }
}
impl CalendarSettings {
    /// Parse the closed calendar settings mapping.
    pub fn parse(value: &Value) -> crate::Result<Self> {
        let bad = |key: &str| crate::Error::InvalidSetting {
            key: format!("calendar.{key}"),
            reason: "expected the documented calendar type and bounds",
        };
        let mut out = Self::default();
        for (key, value) in value.as_object().ok_or_else(|| bad("config"))? {
            if key == "connection" {
                out.connection = if value.is_null() {
                    None
                } else {
                    Some(
                        value
                            .as_str()
                            .filter(|s| {
                                !s.is_empty()
                                    && s.len() <= 48
                                    && s.bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                            })
                            .ok_or_else(|| bad(key))?
                            .into(),
                    )
                };
                continue;
            }
            let (target, min, max) = match key.as_str() {
                "window_days_past" => (&mut out.window_days_past, 1, 90),
                "window_days_ahead" => (&mut out.window_days_ahead, 1, 90),
                "prep_lead_minutes" => (&mut out.prep_lead_minutes, 15, 1440),
                "prep_max_per_run" => (&mut out.prep_max_per_run, 1, 20),
                _ => return Err(bad(key)),
            };
            *target = value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| bad(key))?;
        }
        Ok(out)
    }
}
/// One normalized attendee. Emails are private matching inputs only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attendee {
    /// Display name.
    pub name: String,
    /// Private matching address, omitted from every list/model view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// RSVP state.
    pub response: String,
}
/// Organizer's display name only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Organizer {
    /// Display name.
    pub name: String,
}
/// One event in private plugin state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Authored in this workspace; never synchronized outward.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
    /// UID plus original UTC start, stable when moved.
    pub key: String,
    /// iCalendar UID.
    pub series: String,
    /// Display title.
    pub title: String,
    /// RFC 3339 start.
    pub start: String,
    /// Exclusive RFC 3339 end.
    pub end: String,
    /// Date-only event.
    pub all_day: bool,
    /// confirmed, tentative or cancelled.
    pub status: String,
    /// Location without join URLs or passcodes.
    pub location: String,
    /// Whether a join URL was present; never the URL.
    pub online: bool,
    /// Organizer name only.
    pub organizer: Organizer,
    /// Private attendee matching records.
    pub attendees: Vec<Attendee>,
    /// Untrusted plain text, at most 8 KiB; must be quoted as data.
    pub agenda: String,
    /// normal or private.
    pub sensitivity: String,
    /// Unsupported recurrence preserved only at DTSTART.
    pub recurrence_unsupported: bool,
}
/// Snapshot; errors are persisted separately to preserve its exact bytes on failure.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// Last complete set of events.
    pub events: Vec<Event>,
    /// Unknown or unresolved timezone records skipped.
    pub skipped: usize,
    /// None until a successful sync.
    pub synced_at: Option<String>,
}
/// Half-open range, validated before I/O.
#[derive(Clone, Debug)]
pub struct Window {
    /// Inclusive lower bound.
    pub from: DateTime<FixedOffset>,
    /// Exclusive upper bound.
    pub until: DateTime<FixedOffset>,
}
impl Window {
    /// Expand the sync clock using the validated policy without ambient time.
    pub fn around(now: &str, settings: &CalendarSettings) -> crate::Result<Self> {
        let at = parse_time(now)?;
        let from = at
            .checked_sub_signed(chrono::Duration::days(i64::from(settings.window_days_past)))
            .ok_or(CalendarError::Invalid)?;
        let until = at
            .checked_add_signed(chrono::Duration::days(i64::from(
                settings.window_days_ahead,
            )))
            .ok_or(CalendarError::Invalid)?;
        Ok(Self { from, until })
    }

    /// Construct a nonempty range with an optional maximum day count.
    pub fn new(from: &str, until: &str, max_days: Option<i64>) -> crate::Result<Self> {
        let invalid = || {
            crate::Error::InvalidArgument("calendar from/until must be RFC 3339, ordered, and at most 42 days apart for calendar-list".into())
        };
        let from = parse_time(from).map_err(|_| invalid())?;
        let until = parse_time(until).map_err(|_| invalid())?;
        if until <= from || max_days.is_some_and(|days| until - from > chrono::Duration::days(days))
        {
            return Err(invalid());
        }
        Ok(Self { from, until })
    }
    /// Include events that overlap the range.
    pub fn contains(&self, event: &Event) -> bool {
        parse_time(&event.start).is_ok_and(|s| {
            s < self.until
                && parse_time(&event.end).is_ok_and(|e| e > self.from || (e == s && s >= self.from))
        })
    }
}
/// Strip markup, URLs and passcode lines before persisting external text.
pub fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                tag = true;
                out.push(' ');
            }
            '>' => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .lines()
        .filter(|line| {
            let low = line.to_ascii_lowercase();
            !["passcode", "password", "dial-in", "dial in"]
                .iter()
                .any(|w| low.contains(w))
        })
        .map(|line| {
            line.split_whitespace()
                .filter(|s| !s.contains("://") && !s.starts_with("www."))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut end = out.len().min(8192);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out[..end]
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}
