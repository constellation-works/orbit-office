//! Selections: typed references, a 15-minute expiry and a tray of five
//! (specs/selection-sharing.md).
//!
//! A selection points at one Library object. Private objects store the
//! reference only: the quote and its hash are dropped before anything is
//! recorded, so a hosted model never receives private text from the tray.

use std::time::{Duration, SystemTime};

use office_common::Sensitivity;
use serde_json::{Value, json};

use super::source::{format_rfc3339_utc, sha256_hex};

/// How long a selection stays in the tray. Age is measured from
/// `selected_at`; a selection at exactly this age is expired.
pub const EXPIRY: Duration = Duration::from_secs(15 * 60);

/// Fresh, unconsumed selections the tray holds. A further put evicts the
/// oldest fresh selection.
pub const TRAY_LIMIT: usize = 5;

/// The sensitivity used when a put omits one.
#[must_use]
pub fn default_sensitivity() -> Sensitivity {
    Sensitivity::Normal
}

/// Parse a sensitivity token. Unknown tokens are refused.
#[must_use]
pub fn parse_sensitivity(token: &str) -> Option<Sensitivity> {
    Sensitivity::parse(token)
}

/// Which app view the user pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// A source.
    Source,
    /// A knowledge page.
    Page,
    /// An artifact.
    Artifact,
    /// An Orbit task.
    Task,
}

impl View {
    /// The canonical token stored and returned by the tools.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Page => "page",
            Self::Artifact => "artifact",
            Self::Task => "task",
        }
    }

    /// Parse a canonical token. Unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "source" => Some(Self::Source),
            "page" => Some(Self::Page),
            "artifact" => Some(Self::Artifact),
            "task" => Some(Self::Task),
            _ => None,
        }
    }
}

/// A selection field the caller sent that cannot be stored.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectionError {
    /// `field` is missing or empty.
    #[error("invalid selection field {field}")]
    Invalid {
        /// The refused field name.
        field: &'static str,
    },
}

/// A selection the caller asked to record, before id and time are assigned.
///
/// Private selections already have no quote. [`normalize`] is the function
/// that enforces that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSelection {
    /// Source, page, artifact or task id.
    pub object: String,
    /// Anchor inside the object, or the whole object when absent.
    pub anchor: Option<String>,
    /// Quoted text. Always absent for a private object.
    pub quote: Option<String>,
    /// Hash the app recorded so the agent can detect a later edit.
    /// Always absent for a private object.
    pub quote_sha256: Option<String>,
    /// The view the user was looking at.
    pub view: View,
    /// How far the quoted text may travel.
    pub sensitivity: Sensitivity,
}

/// One stored selection, including its id and the time it was recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Stable id `sel-<n>`.
    pub id: String,
    /// Source, page, artifact or task id.
    pub object: String,
    /// Anchor inside the object, or the whole object when absent.
    pub anchor: Option<String>,
    /// Quoted text. Absent for a private object.
    pub quote: Option<String>,
    /// Caller-supplied staleness hash. Absent for a private object.
    pub quote_sha256: Option<String>,
    /// The view the user was looking at.
    pub view: View,
    /// How far the quoted text may travel.
    pub sensitivity: Sensitivity,
    /// When the app recorded the selection.
    pub selected_at: SystemTime,
}

/// What `office.selection-put` asks the tray to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PutRequest {
    /// Empty the tray before recording [`Self::selection`].
    pub clear: bool,
    /// The selection to record, if this call is not clear-only.
    pub selection: Option<NewSelection>,
}

/// What a put changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PutResult {
    /// True when the call asked to empty the tray.
    pub cleared: bool,
    /// Ids removed so the new selection fit in the tray, oldest first.
    pub evicted: Vec<String>,
    /// The selection that was recorded, when the call included one.
    pub selection: Option<Selection>,
}

/// One row the tray stores. Consumed and expired rows stay so a later get
/// can prove it will not return them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayRow {
    /// Monotonic sequence. The id is `sel-<seq>`.
    pub seq: u64,
    /// The selection.
    pub selection: Selection,
    /// When get handed the selection to the agent.
    pub consumed_at: Option<SystemTime>,
}

/// The tray rules, independent of SQLite. Adapters load rows, apply this,
/// and write the rows back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tray {
    /// Every row still on disk, including consumed and expired ones.
    pub rows: Vec<TrayRow>,
}

impl Tray {
    /// Record `request` at `now`.
    #[must_use]
    pub fn put(&mut self, request: &PutRequest, now: SystemTime) -> PutResult {
        if request.clear {
            self.rows.clear();
        }
        let selection = request.selection.as_ref().map(|draft| {
            let evicted = eviction_ids(&self.fresh(now));
            if !evicted.is_empty() {
                self.rows
                    .retain(|row| !evicted.iter().any(|id| id == &row.selection.id));
            }
            let seq = self.rows.iter().map(|row| row.seq).max().unwrap_or(0) + 1;
            let (quote, quote_sha256) = if draft.sensitivity == Sensitivity::Private {
                (None, None)
            } else {
                (draft.quote.clone(), draft.quote_sha256.clone())
            };
            let selection = Selection {
                id: format!("sel-{seq}"),
                object: draft.object.clone(),
                anchor: draft.anchor.clone(),
                quote,
                quote_sha256,
                view: draft.view,
                sensitivity: draft.sensitivity,
                selected_at: now,
            };
            self.rows.push(TrayRow {
                seq,
                selection: selection.clone(),
                consumed_at: None,
            });
            (evicted, selection)
        });
        let (evicted, selection) = selection.map_or((Vec::new(), None), |(evicted, selection)| {
            (evicted, Some(selection))
        });
        PutResult {
            cleared: request.clear,
            evicted,
            selection,
        }
    }

    /// Fresh, unconsumed selections in tray order (oldest first). Marks
    /// each returned row consumed at `now`.
    #[must_use]
    pub fn get(&mut self, now: SystemTime) -> Vec<Selection> {
        let chosen = self.peek(now);
        let ids: Vec<String> = chosen
            .iter()
            .map(|selection| selection.id.clone())
            .collect();
        for row in &mut self.rows {
            if ids.iter().any(|id| id == &row.selection.id) {
                row.consumed_at = Some(now);
            }
        }
        chosen
    }

    /// Fresh, unconsumed selections in tray order without consuming them.
    #[must_use]
    pub fn peek(&self, now: SystemTime) -> Vec<Selection> {
        let mut chosen = self.fresh(now);
        chosen.sort_by_key(|selection| selection.selected_at);
        chosen
    }

    fn fresh(&self, now: SystemTime) -> Vec<Selection> {
        self.rows
            .iter()
            .filter(|row| row.consumed_at.is_none() && !expired(row.selection.selected_at, now))
            .map(|row| row.selection.clone())
            .collect()
    }
}

/// True when `selected_at` is at least [`EXPIRY`] before `now`.
#[must_use]
pub fn expired(selected_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(selected_at)
        .is_ok_and(|age| age >= EXPIRY)
}

/// Ids to drop so one more fresh selection fits, oldest first.
#[must_use]
pub fn eviction_ids(fresh: &[Selection]) -> Vec<String> {
    if fresh.len() < TRAY_LIMIT {
        return Vec::new();
    }
    let mut ordered: Vec<&Selection> = fresh.iter().collect();
    ordered.sort_by_key(|selection| selection.selected_at);
    let drop_count = fresh.len() + 1 - TRAY_LIMIT;
    ordered
        .into_iter()
        .take(drop_count)
        .map(|selection| selection.id.clone())
        .collect()
}

/// Check a caller-supplied selection and drop any quote on a private object.
///
/// An omitted `quote_sha256` on a normal object is the SHA-256 of the quote.
/// A private object stores neither, even when the caller sent both.
pub fn normalize(
    object: &str,
    anchor: Option<String>,
    quote: Option<String>,
    quote_sha256: Option<String>,
    view: View,
    sensitivity: Sensitivity,
) -> Result<NewSelection, SelectionError> {
    let object = object.trim();
    if object.is_empty() {
        return Err(SelectionError::Invalid { field: "object" });
    }
    let anchor = match anchor {
        Some(value) if value.trim().is_empty() => {
            return Err(SelectionError::Invalid { field: "anchor" });
        }
        Some(value) => Some(value),
        None => None,
    };
    if sensitivity == Sensitivity::Private {
        return Ok(NewSelection {
            object: object.to_string(),
            anchor,
            quote: None,
            quote_sha256: None,
            view,
            sensitivity,
        });
    }
    let quote = match quote {
        Some(value) if value.is_empty() => None,
        other => other,
    };
    let quote_sha256 = match quote_sha256 {
        Some(value) if value.is_empty() => {
            return Err(SelectionError::Invalid {
                field: "quote_sha256",
            });
        }
        Some(value) => Some(value),
        None => quote.as_deref().map(|text| sha256_hex(text.as_bytes())),
    };
    Ok(NewSelection {
        object: object.to_string(),
        anchor,
        quote,
        quote_sha256,
        view,
        sensitivity,
    })
}

/// The JSON object both selection tools return for one selection.
#[must_use]
pub fn to_json(selection: &Selection) -> Value {
    json!({
        "id": selection.id,
        "object": selection.object,
        "anchor": selection.anchor,
        "quote": selection.quote,
        "quote_sha256": selection.quote_sha256,
        "view": selection.view.as_str(),
        "sensitivity": selection.sensitivity.as_str(),
        "selected_at": format_rfc3339_utc(selection.selected_at),
    })
}
