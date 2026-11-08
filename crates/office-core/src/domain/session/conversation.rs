//! Conversation sources parsed for the brief.
use std::collections::BTreeMap;

use serde_json::Value;

use crate::Result;
use crate::domain::knowledge::frontmatter;
use crate::domain::metrics::metadata;
use crate::domain::source::LibraryText;

use super::entry::{Entry, entries};
use super::helpers::cites;
use super::helpers::{markdown, optional_text, text, time_key};
use super::item::Item;
use super::scope::stored_scope;
use super::threads::GlobalHandoff;
use super::{handoff, scope::GlobalScope};

pub(crate) struct Conversation {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) time: String,
    pub(crate) log: Vec<Entry>,
    pub(crate) project: Option<String>,
    pub(crate) orchestrator: Option<String>,
    pub(crate) session_id: Option<String>,
    /// `sensitivity: private`: the list view withholds its entry text.
    pub(crate) private: bool,
    /// Lines of original `handoff` entries that declared `scope: global`.
    /// Read from the original log, so a correction cannot change a scope.
    pub(crate) globals: BTreeMap<usize, GlobalScope>,
}

impl Conversation {
    /// The newest handoff in this log that belongs to its project thread.
    pub(crate) fn project_handoff(&self) -> Option<&Entry> {
        self.log
            .iter()
            .rev()
            .find(|e| e.kind == "handoff" && !self.globals.contains_key(&e.line))
    }
    /// Whether `entry` (in `self`) cites the global handoff `global` by
    /// anchor: its full id from anywhere, or its local anchor in its own log.
    pub(crate) fn entry_cites(&self, entry: &Entry, global: &GlobalHandoff<'_>) -> bool {
        self.entry_cites_line(entry, global.conversation, global.entry)
    }
    /// Whether `entry` (in `self`) cites `target` (in `owner`) by anchor.
    pub(crate) fn entry_cites_line(
        &self,
        entry: &Entry,
        owner: &Conversation,
        target: &Entry,
    ) -> bool {
        cites(&entry.text, &format!("{}#L{}", owner.id, target.line))
            || (self.id == owner.id && cites(&entry.text, &format!("#L{}", target.line)))
    }
    pub(crate) fn item(&self, entry: &Entry) -> Item {
        // conversations() validates the effective entries before projection.
        let structured_handoff = (entry.kind == "handoff")
            .then(|| handoff::decode(&entry.text).ok().flatten())
            .flatten();
        let legacy = (entry.kind == "handoff" && structured_handoff.is_none()).then_some(true);
        let text = structured_handoff
            .as_ref()
            .map_or_else(|| entry.text.clone(), |h| handoff::render(&entry.text, h));
        Item {
            id: format!("{}#L{}", self.id, entry.line),
            link: format!("{}#L{}", self.path, entry.line),
            text,
            time: Some(entry.time.clone()),
            entry_kind: Some(entry.kind.clone()),
            status: None,
            template: None,
            workspace: None,
            corrected: entry.corrected.then_some(true),
            by: entry.by.clone(),
            structured_handoff,
            legacy,
            listing: None,
        }
    }
    pub(crate) fn reference(&self) -> Item {
        Item {
            id: self.id.clone(),
            link: self.path.clone(),
            text: self.title.clone(),
            time: Some(self.time.clone()),
            entry_kind: None,
            status: None,
            template: None,
            workspace: None,
            corrected: None,
            by: None,
            structured_handoff: None,
            legacy: None,
            listing: None,
        }
    }
}

pub(crate) fn conversations(files: &[LibraryText]) -> Result<Vec<Conversation>> {
    let mut result = Vec::new();
    for file in files {
        let raw = markdown(file)?;
        let fields = Value::Object(metadata(raw)?);
        if text(&fields, "kind")? != "conversation" {
            continue;
        }
        let (_, body) = frontmatter::split(raw)?;
        let log = crate::domain::source::conversation::corrections::effective_entries(body)?;
        let mut time = text(&fields, "captured_at")?.to_string();
        let mut key = time_key(&time)?;
        let mut globals = BTreeMap::new();
        // Activity includes correction/removal audit records even when the
        // effective evidence is empty or retains an older timestamp.
        for entry in entries(body)? {
            let next = time_key(&entry.time)?;
            if next >= key {
                time.clone_from(&entry.time);
                key = next;
            }
            if entry.kind == "handoff"
                && let Some(scope) = stored_scope(&entry.text)?
            {
                globals.insert(entry.line, scope);
            }
        }
        result.push(Conversation {
            id: text(&fields, "id")?.into(),
            title: text(&fields, "title")?.into(),
            path: file.path.clone(),
            time,
            log,
            project: optional_text(&fields, "project")?,
            orchestrator: optional_text(&fields, "orchestrator")?,
            session_id: optional_text(&fields, "session_id")?,
            private: optional_text(&fields, "sensitivity")?.as_deref() == Some("private"),
            globals,
        });
    }
    // Keys were validated above; sorting never maps an unknown timestamp to zero.
    let mut keyed = result
        .into_iter()
        .map(|c| Ok((time_key(&c.time)?, c)))
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| y.id.cmp(&x.id)));
    Ok(keyed.into_iter().map(|(_, c)| c).collect())
}
