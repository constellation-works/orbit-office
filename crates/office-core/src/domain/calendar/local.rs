//! Workspace-authored events and source selection; no outward operations.
use super::{Attendee, CalendarError, Event, Organizer, parse_time, plain};
use serde::{Deserialize, Serialize};

/// Persisted calendar selection; disabling a source preserves its events.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarSources {
    /// Include workspace-authored events.
    pub local: bool,
    /// Include the external snapshot.
    pub external: bool,
}
impl Default for CalendarSources {
    fn default() -> Self {
        Self {
            local: false,
            external: true,
        }
    }
}

/// Local event input. Identity and provenance are assigned by the store.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvent {
    /// Nonempty display title, at most 512 bytes.
    pub title: String,
    /// Inclusive RFC 3339 start.
    pub start: String,
    /// Exclusive RFC 3339 end, after start.
    pub end: String,
    /// Date-only entry.
    #[serde(default)]
    pub all_day: bool,
    /// Display location, without join URLs.
    #[serde(default)]
    pub location: String,
    /// Untrusted plain text agenda.
    #[serde(default)]
    pub agenda: String,
    /// Attendee display names; no addresses are needed for person matching.
    #[serde(default)]
    pub attendees: Vec<String>,
    /// Uses the existing private inference route when enabled.
    #[serde(default)]
    pub private: bool,
}
impl LocalEvent {
    /// Validate before acquiring a writer lock or touching storage.
    pub fn validate(&self) -> crate::Result<()> {
        let start = parse_time(&self.start)?;
        let end = parse_time(&self.end)?;
        if plain(self.title.trim()).is_empty()
            || self.title.len() > 512
            || self.location.len() > 1024
            || self.agenda.len() > 8192
            || self.attendees.len() > 100
            || self
                .attendees
                .iter()
                .any(|a| a.trim().is_empty() || a.len() > 256 || a.contains(['\n', '\r']))
            || end <= start
            || (self.all_day
                && (start.time() != chrono::NaiveTime::MIN
                    || end.time() != chrono::NaiveTime::MIN
                    || start.offset().local_minus_utc() != 0
                    || end.offset().local_minus_utc() != 0))
        {
            return Err(CalendarError::Invalid.into());
        }
        Ok(())
    }
    /// Normalize local content through the same plain-text policy as imports.
    pub fn event(&self, key: String) -> Event {
        Event {
            local: true,
            series: key.clone(),
            key,
            title: plain(self.title.trim()),
            start: self.start.clone(),
            end: self.end.clone(),
            all_day: self.all_day,
            status: "confirmed".into(),
            location: plain(&self.location),
            agenda: plain(&self.agenda),
            online: false,
            organizer: Organizer {
                name: String::new(),
            },
            attendees: self
                .attendees
                .iter()
                .map(|name| Attendee {
                    name: plain(name.trim()),
                    email: None,
                    response: "needsAction".into(),
                })
                .collect(),
            sensitivity: if self.private { "private" } else { "normal" }.into(),
            recurrence_unsupported: false,
        }
    }
}

/// One atomic local mutation, carried by calendar-sync without a gateway call.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalChange {
    /// Create with a store-assigned stable key.
    Create {
        /// Validated event fields.
        event: LocalEvent,
    },
    /// Replace fields without changing the event or series key.
    Update {
        /// Stable local event key.
        key: String,
        /// Replacement event fields.
        event: LocalEvent,
    },
    /// Remove only a local event; existing meeting sources remain in the Library.
    Delete {
        /// Stable local event key.
        key: String,
    },
    /// Select either source or both, without deleting any data.
    Sources {
        /// Include authored events.
        local: bool,
        /// Include the imported snapshot.
        external: bool,
    },
}
impl LocalChange {
    /// Reject malformed edits before storage access.
    pub fn validate(&self) -> crate::Result<()> {
        match self {
            Self::Create { event } => event.validate(),
            Self::Update { key, event } => {
                local_key(key)?;
                event.validate()
            }
            Self::Delete { key } => local_key(key),
            Self::Sources { .. } => Ok(()),
        }
    }
}
fn local_key(key: &str) -> crate::Result<()> {
    if key.starts_with("local-")
        && key.len() <= 128
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Ok(())
    } else {
        Err(CalendarError::Invalid.into())
    }
}
/// Workspace-owned local state, versioned independently of imported snapshots.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalCalendar {
    /// Current disk format.
    pub schema_version: u32,
    /// Persistent source choices.
    pub sources: CalendarSources,
    /// Events authored here.
    pub events: Vec<Event>,
}
impl Default for LocalCalendar {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sources: CalendarSources::default(),
            events: Vec::new(),
        }
    }
}
impl LocalCalendar {
    /// Apply under the adapter's writer lock. Unknown keys cannot create events.
    pub fn apply(
        &mut self,
        change: &LocalChange,
        new_key: String,
    ) -> crate::Result<Option<String>> {
        change.validate()?;
        match change {
            LocalChange::Create { event } => {
                if self.events.len() >= 10000 {
                    return Err(CalendarError::TooLarge.into());
                }
                local_key(&new_key)?;
                if self.events.iter().any(|e| e.key == new_key) {
                    return Err(CalendarError::Invalid.into());
                }
                self.events.push(event.event(new_key.clone()));
                self.sources.local = true;
                Ok(Some(new_key))
            }
            LocalChange::Update { key, event } => {
                let current = self
                    .events
                    .iter_mut()
                    .find(|e| e.key == *key)
                    .ok_or(CalendarError::Invalid)?;
                *current = event.event(key.clone());
                Ok(Some(key.clone()))
            }
            LocalChange::Delete { key } => {
                let index = self
                    .events
                    .iter()
                    .position(|e| e.key == *key)
                    .ok_or(CalendarError::Invalid)?;
                self.events.remove(index);
                Ok(Some(key.clone()))
            }
            LocalChange::Sources { local, external } => {
                self.sources = CalendarSources {
                    local: *local,
                    external: *external,
                };
                Ok(None)
            }
        }
    }
}
