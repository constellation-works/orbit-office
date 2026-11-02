//! Per-session scribe control state (specs/sessions.md, "Live scribe";
//! specs/recall.md, "Controls").
//!
//! Office owns one small record per provider session: the scribe switch, a
//! pending **Capture now** request, the user's title and the last Scribe
//! window heartbeat. The web window and the provider plugin's hooks both
//! read and write it through `office.scribe-control`, so they share one
//! switch. It is plugin state, not Library evidence: nothing here is
//! captured, indexed or synced. These rules are shared by the SQLite
//! adapter and the test fake so they cannot drift.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::source::format_rfc3339_utc;
use crate::{Error, Result};

/// A heartbeat younger than this means a Scribe window is open on the
/// session, and the end-of-turn hook uses its tighter turn bound.
pub const WINDOW_OPEN: Duration = Duration::from_secs(120);

/// Records untouched for this long are dropped on the next write.
pub const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Longest accepted session identity, matching the provider plugin.
pub const SESSION_ID_MAX: usize = 128;

/// Longest accepted title, matching session-mode capture context.
pub const TITLE_MAX: usize = 4096;

/// Who turned scribing on for a session (specs/scribe.md, "Turning it
/// on"): the user (`/scribe on`, the window, the pane) or an orchestrator
/// agent in an interactive session (the plugin's `scribe` tool).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnBy {
    /// The user turned it on.
    User,
    /// An orchestrator agent turned it on; it must announce that.
    Agent,
}

impl OnBy {
    /// `user` or `agent`, as stored and returned.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    /// Parse the stored or requested word; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "user" => Some(Self::User),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

/// Longest accepted scope token (`global` or `project:<name>`).
pub const SCOPE_MAX: usize = 72;

/// Refuse a scope token the scribe pass could not have produced: `global`
/// or `project:<name>` with a lowercase kebab-case name, the tokens
/// `office.scribe-pending` resolves (specs/scribe.md, "Session scope").
pub fn validate_scope(value: &str) -> Result<()> {
    let name = if value == "global" {
        return Ok(());
    } else if let Some(name) = value.strip_prefix("project:") {
        name
    } else {
        return Err(Error::InvalidArgument(
            "scope must be global or project:<name>".into(),
        ));
    };
    let kebab = !name.is_empty()
        && name.len() + "project:".len() <= SCOPE_MAX
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || (byte == b'-' && index > 0)
        })
        && !name.ends_with('-')
        && !name.contains("--");
    if kebab {
        Ok(())
    } else {
        Err(Error::InvalidArgument(
            "scope must be global or project:<name> with a lowercase kebab-case name".into(),
        ))
    }
}

/// One session's control record. A missing record reads as the default:
/// switch unset (`None`, so the plugin's default applies), nothing pending.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Control {
    /// The scribe switch, once someone set it.
    pub on: Option<bool>,
    /// Who turned scribing on; `None` while off or never turned on.
    pub on_by: Option<OnBy>,
    /// The session's scope token (`global` or `project:<name>`), once a
    /// turn-on named one. Kept across `off` so status still names it.
    pub scope: Option<String>,
    /// When an unconsumed Capture-now request was made.
    pub now_requested_at: Option<SystemTime>,
    /// The user's title for the session, applied by the plugin through capture.
    pub title: Option<String>,
    /// The last Scribe window heartbeat.
    pub heartbeat_at: Option<SystemTime>,
    /// The last change to this record.
    pub updated_at: Option<SystemTime>,
}

/// A switch change: the new state, and on a turn-on who asked and the
/// scope it named (both optional for pre-v2 callers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    /// The new switch state.
    pub on: bool,
    /// Who is turning it on; ignored on a turn-off.
    pub on_by: Option<OnBy>,
    /// The scope token the turn-on named; ignored on a turn-off.
    pub scope: Option<String>,
}

impl Switch {
    /// A plain switch change with no v2 fields (pre-v2 callers).
    #[must_use]
    pub fn plain(on: bool) -> Self {
        Self {
            on,
            on_by: None,
            scope: None,
        }
    }
}

/// A requested change to one session's record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Turn the scribe on or off.
    SetSwitch(Switch),
    /// Record a pending Capture-now request (an existing one is kept).
    RequestNow,
    /// Clear the pending request. With `Some(at)`, clear it only when it is
    /// still the request made at `at`, so a newer request is never lost.
    ConsumeNow(Option<SystemTime>),
    /// Set the user's title.
    SetTitle(String),
    /// Record that a Scribe window is open now.
    Heartbeat,
}

/// Refuse a session identity the provider plugin could not have produced.
pub fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > SESSION_ID_MAX
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::InvalidArgument(
            "session_id must be 1–128 ASCII letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}

/// Refuse an empty, multi-line or oversized title.
pub fn validate_title(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > TITLE_MAX || value.chars().any(char::is_control) {
        return Err(Error::InvalidArgument(
            "title must be a nonempty single line of at most 4096 bytes".into(),
        ));
    }
    Ok(())
}

/// Seconds since the epoch, the stored precision.
#[must_use]
pub fn to_unix(time: SystemTime) -> Option<i64> {
    i64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()
}

/// The time `secs` after the epoch; `None` for a negative value.
#[must_use]
pub fn from_unix(secs: i64) -> Option<SystemTime> {
    u64::try_from(secs)
        .ok()
        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
}

/// Parse an RFC 3339 timestamp as returned in `now_requested_at`.
pub fn parse_time(text: &str) -> Result<SystemTime> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .and_then(|at| from_unix(at.timestamp()))
        .ok_or_else(|| {
            Error::InvalidArgument("requested_at must be an RFC 3339 timestamp after 1970".into())
        })
}

/// Truncate to whole seconds, so a fresh value compares equal to a stored one.
fn whole_seconds(time: SystemTime) -> SystemTime {
    to_unix(time).and_then(from_unix).unwrap_or(UNIX_EPOCH)
}

impl Control {
    /// Apply `change` at `now`. Returns whether a pending Capture-now
    /// request was consumed (always false except for [`Change::ConsumeNow`]).
    pub fn apply(&mut self, change: &Change, now: SystemTime) -> Result<bool> {
        let now = whole_seconds(now);
        let mut consumed = false;
        match change {
            Change::SetSwitch(switch) => {
                if let Some(scope) = &switch.scope {
                    validate_scope(scope)?;
                }
                self.on = Some(switch.on);
                if switch.on {
                    // A caller that names nobody keeps the recorded
                    // turn-on, else reads as the user's (the window and
                    // pane predate the field and are the user's hosts).
                    if switch.on_by.is_some() || self.on_by.is_none() {
                        self.on_by = Some(switch.on_by.unwrap_or(OnBy::User));
                    }
                    if switch.scope.is_some() {
                        self.scope.clone_from(&switch.scope);
                    }
                } else {
                    self.on_by = None;
                }
            }
            Change::RequestNow => {
                self.now_requested_at.get_or_insert(now);
            }
            Change::ConsumeNow(expected) => {
                let matches =
                    expected.is_none_or(|at| Some(whole_seconds(at)) == self.now_requested_at);
                if self.now_requested_at.is_some() && matches {
                    self.now_requested_at = None;
                    consumed = true;
                }
            }
            Change::SetTitle(title) => {
                validate_title(title)?;
                self.title = Some(title.trim().to_owned());
            }
            Change::Heartbeat => self.heartbeat_at = Some(now),
        }
        self.updated_at = Some(now);
        Ok(consumed)
    }

    /// Whether a Scribe window heartbeat is fresh at `now`. A heartbeat
    /// stamped slightly ahead of `now` (clock adjustment) still counts.
    #[must_use]
    pub fn window_open(&self, now: SystemTime) -> bool {
        self.heartbeat_at
            .is_some_and(|at| now.duration_since(at).map_or(true, |age| age < WINDOW_OPEN))
    }

    /// Whether this record is old enough to drop at `now`.
    #[must_use]
    pub fn expired(&self, now: SystemTime) -> bool {
        self.updated_at
            .is_some_and(|at| now.duration_since(at).is_ok_and(|age| age >= RETENTION))
    }

    /// The tool's view of this record.
    #[must_use]
    pub fn view(&self, session_id: &str, now: SystemTime) -> Value {
        let time = |t: Option<SystemTime>| t.map(format_rfc3339_utc);
        json!({
            "session_id": session_id,
            "on": self.on,
            "on_by": self.on_by.map(OnBy::as_str),
            "scope": self.scope,
            "now_requested": self.now_requested_at.is_some(),
            "now_requested_at": time(self.now_requested_at),
            "title": self.title,
            "heartbeat_at": time(self.heartbeat_at),
            "window_open": self.window_open(now),
            "updated_at": time(self.updated_at),
        })
    }
}
