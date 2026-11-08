use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::decision::Decision;
use super::ledger::{Ledger, LedgerEntry};
use super::matching::parse_entry_id;
use super::scope::is_ledger_path;
use crate::domain::knowledge::{DocumentKind, DocumentRecord, LibraryLayer};

/// An entry as read: the written fields plus the derived ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryView {
    /// The written entry.
    #[serde(flatten)]
    pub decision: Decision,
    /// The first later entry whose `reverses` names this one; never written.
    pub reversed_by: Option<String>,
    /// The ledger's id.
    pub ledger: String,
    /// The entry's 1-based line in the ledger body.
    pub line: usize,
}

/// Every entry across `ledgers`, with `reversed_by` derived from every
/// ledger's `reverses`. Ledger order, then line order.
#[must_use]
pub fn views(ledgers: &[Ledger]) -> Vec<EntryView> {
    let mut reversed: BTreeMap<&str, &str> = BTreeMap::new();
    for ledger in ledgers {
        for entry in &ledger.entries {
            if let Some(target) = entry.decision.reverses.as_deref() {
                reversed.entry(target).or_insert(entry.decision.id.as_str());
            }
        }
    }
    ledgers
        .iter()
        .flat_map(|ledger| {
            let reversed = &reversed;
            ledger.entries.iter().map(move |entry| EntryView {
                decision: entry.decision.clone(),
                reversed_by: reversed
                    .get(entry.decision.id.as_str())
                    .map(|id| (*id).to_owned()),
                ledger: ledger.id(),
                line: entry.line,
            })
        })
        .collect()
}

/// What an id names among the ledgers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// A ledger by its `decisions-<scope>` id.
    Ledger(&'a Ledger),
    /// One entry, by its `dec-` id or a migrated source's former id
    /// (`moved_from`).
    Entry {
        /// The ledger holding it.
        ledger: &'a Ledger,
        /// The entry.
        entry: &'a LedgerEntry,
        /// The migrated source id the lookup used, if any.
        moved_from: Option<String>,
    },
}

/// Resolve a ledger id, an entry id or a migrated source id.
#[must_use]
pub fn resolve<'a>(ledgers: &'a [Ledger], id: &str) -> Option<Resolved<'a>> {
    if let Some(ledger) = ledgers.iter().find(|ledger| ledger.id() == id) {
        return Some(Resolved::Ledger(ledger));
    }
    if parse_entry_id(id).is_some() {
        return ledgers.iter().find_map(|ledger| {
            ledger.entry(id).map(|entry| Resolved::Entry {
                ledger,
                entry,
                moved_from: None,
            })
        });
    }
    ledgers.iter().find_map(|ledger| {
        let target = ledger.moved_sources.get(id)?;
        ledger.entry(target).map(|entry| Resolved::Entry {
            ledger,
            entry,
            moved_from: Some(id.to_owned()),
        })
    })
}

/// The document record a citation check sees for an entry: id as cited
/// (entry or former source id), the ledger's path, and the entry's verbatim
/// `quote` as the body, so a former `src-…#L<n>` anchor still names a line of
/// the text it cited. A private entry has an empty body.
#[must_use]
pub fn entry_record(ledger: &Ledger, entry: &LedgerEntry, cited_id: &str) -> DocumentRecord {
    let decision = &entry.decision;
    DocumentRecord {
        layer: LibraryLayer::Own,
        shared_by: None,
        id: cited_id.into(),
        kind: DocumentKind::Source,
        title: decision
            .question
            .clone()
            .unwrap_or_else(|| decision.id.clone()),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".into(),
        shareable: false,
        path: ledger.path.clone(),
        body: decision.quote.clone().unwrap_or_default(),
        cites: Vec::new(),
        related: Vec::new(),
        template: None,
        date: decision.decided_at.get(..10).map(str::to_owned),
    }
}

/// Whether a record stands for one ledger entry (cited bare), rather than a
/// ledger or an ordinary source.
#[must_use]
pub fn is_entry_record(record: &DocumentRecord) -> bool {
    is_ledger_path(Path::new(&record.path)) && !record.id.starts_with("decisions-")
}
