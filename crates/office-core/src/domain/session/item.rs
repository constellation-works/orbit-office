//! Brief item, section and brief shapes.
use serde::Serialize;

use super::{handoff, listing};

/// One cited data item. `time` stays null for undated template suggestions.
#[derive(Debug, Serialize)]
pub struct Item {
    /// Stable object id, with a body anchor for an entry.
    pub id: String,
    /// Library-relative path (with optional anchor), or an Orbit task URI.
    pub link: String,
    /// Verbatim ordinary entry or object title; handoffs render structured
    /// fields in order while retaining scope and scribe markers.
    pub text: String,
    /// Recorded timestamp; never manufactured from the current clock.
    pub time: Option<String>,
    /// The capture kind for a quoted entry, else null.
    pub entry_kind: Option<String>,
    /// Task lifecycle state or typed run failure, else null.
    pub status: Option<String>,
    /// Template name for an onboarding suggestion, else null.
    pub template: Option<String>,
    /// Orbit workspace selector for a task read from an extra workspace
    /// (`brief.task_workspaces`); absent for the office workspace, so
    /// existing briefs serialize unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Additive correction marker, present only for corrected entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected: Option<bool>,
    /// Explicit user attribution on a captured entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Structured handoff data, absent on ordinary and historical entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_handoff: Option<handoff::Structured>,
    /// Historical unstructured handoff; its original text is retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy: Option<bool>,
    /// Thread, scope and state of a handoff in the list view
    /// (`view: list`); absent from every other output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<listing::Listing>,
}

/// A section in display order; optional sections are absent from the array.
#[derive(Debug, Serialize)]
pub struct Section {
    /// Stable section token documented by the response schema.
    pub kind: &'static str,
    /// Cited records, possibly empty.
    pub items: Vec<Item>,
    /// Matching items before the last-session excerpt limit.
    pub total: usize,
    /// Whether an explicit excerpt limit omitted records.
    pub truncated: bool,
    /// The page's first index in the list view's paged `history`; absent
    /// from every other section.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Section {
    pub(crate) fn new(kind: &'static str, items: Vec<Item>) -> Self {
        Self {
            kind,
            total: items.len(),
            items,
            truncated: false,
            offset: None,
        }
    }
}

/// A complete brief; section order is part of the tool contract.
#[derive(Debug, Serialize)]
pub struct Brief {
    /// Handoff, Global, Projects, Other sessions, Since, Waiting, Agents,
    /// Questions, Last session, Digest, Onboarding.
    pub sections: Vec<Section>,
}
