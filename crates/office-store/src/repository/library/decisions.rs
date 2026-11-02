//! Decision ledgers in the Library store (specs/scribe.md, Decision
//! ledger): resolving ledger, entry and migrated note ids, and appending
//! one entry under the writer lock.

use office_core::domain::source::decisions::{
    Decision, Ledger, Resolved, append_line, entry_record, parse_entry_id, resolve, views,
};
use office_core::domain::source::source_body;
use office_core::ports::{DecisionOutcome, SourceView};

use super::LibraryStore;
use super::pending::WriteKind;
use crate::Result;
use crate::contracts::DocumentRecord;
use crate::driver::file::lock::WriterLock;

impl LibraryStore {
    /// Every ledger, parsed, beside its file text. A malformed ledger is
    /// refused rather than skipped, so a lookup never silently misses it.
    pub(super) fn ledgers(&self) -> Result<Vec<(Ledger, String)>> {
        let mut out = Vec::new();
        for (path, text) in self.files.read_ledger_markdown()? {
            out.push((Ledger::parse(&path, &text)?, text));
        }
        Ok(out)
    }

    /// The record a citation check sees for a `dec-` entry id or a
    /// migrated note's former `src-` id, when no indexed document has it.
    pub(super) fn ledger_record(&self, id: &str) -> Result<Option<DocumentRecord>> {
        if !(id.starts_with("dec-") || id.starts_with("src-")) {
            return Ok(None);
        }
        let ledgers: Vec<Ledger> = self.ledgers()?.into_iter().map(|(l, _)| l).collect();
        Ok(match resolve(&ledgers, id) {
            Some(Resolved::Entry { ledger, entry, .. }) => Some(entry_record(ledger, entry, id)),
            Some(Resolved::Ledger(_)) | None => None,
        })
    }

    /// `office.source-show`'s view of a ledger (`decisions-<scope>`, every
    /// entry), an entry (`dec-<scope>-<n>`) or a migrated note id (its
    /// entry, with `moved_from`). `None` for any other id.
    pub(super) fn ledger_view(&self, id: &str) -> office_core::Result<Option<SourceView>> {
        let ledger_shaped = id.starts_with("decisions-") || parse_entry_id(id).is_some();
        // A `src-` id reaches the ledgers only when no indexed source has it:
        // it can then be a migrated note's alias.
        let moved_alias = id.starts_with("src-") && self.index.get(id)?.is_none();
        if !(ledger_shaped || moved_alias) {
            return Ok(None);
        }
        let ledgers = self.ledgers()?;
        let parsed: Vec<Ledger> = ledgers.iter().map(|(l, _)| l.clone()).collect();
        let all = views(&parsed);
        let view = |id: String, title: String, path: &str, body: String| SourceView {
            id,
            kind: office_core::domain::source::decisions::KIND.into(),
            title,
            path: path.into(),
            body,
            sensitivity: "normal".into(),
            sha256: None,
            bytes: None,
            media_type: None,
            version: 1,
            decisions: Vec::new(),
            moved_from: None,
        };
        Ok(match resolve(&parsed, id) {
            None => None,
            Some(Resolved::Ledger(ledger)) => {
                let text = ledgers
                    .iter()
                    .find(|(l, _)| l.path == ledger.path)
                    .map(|(_, text)| text.as_str())
                    .unwrap_or_default();
                let mut out = view(
                    ledger.id(),
                    ledger.title(),
                    &ledger.path,
                    source_body(text).trim_end_matches('\n').to_string(),
                );
                out.decisions = all
                    .into_iter()
                    .filter(|entry| entry.ledger == ledger.id())
                    .collect();
                Some(out)
            }
            Some(Resolved::Entry {
                ledger,
                entry,
                moved_from,
            }) => {
                let decision = &entry.decision;
                let mut out = view(
                    decision.id.clone(),
                    decision
                        .question
                        .clone()
                        .unwrap_or_else(|| decision.id.clone()),
                    &ledger.path,
                    decision.quote.clone().unwrap_or_default(),
                );
                out.decisions = all
                    .into_iter()
                    .filter(|view| view.decision.id == decision.id)
                    .collect();
                out.moved_from = moved_from;
                Some(out)
            }
        })
    }

    /// Append one entry to its scope's ledger and commit it.
    pub(super) fn append_ledger_entry(
        &self,
        decision: &Decision,
        captured_at: &str,
    ) -> office_core::Result<DecisionOutcome> {
        self.append_ledger_entries(std::slice::from_ref(decision), captured_at)?
            .pop()
            .ok_or(office_core::Error::LibraryUnavailable)
    }

    /// Append every entry, in order, to their one scope's ledger in one
    /// commit. Every check runs before the write, so a failing entry
    /// leaves the ledger untouched (an accepted `ledger_append` proposal).
    pub(super) fn append_ledger_entries(
        &self,
        decisions: &[Decision],
        captured_at: &str,
    ) -> office_core::Result<Vec<DecisionOutcome>> {
        let first = decisions
            .first()
            .ok_or_else(|| office_core::Error::Library {
                code: "invalid_decision",
                message: "a ledger append needs at least one entry".into(),
            })?;
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let scope = first.parsed_scope()?;
        if decisions
            .iter()
            .any(|decision| decision.scope != scope.token())
        {
            return Err(office_core::Error::Library {
                code: "invalid_decision",
                message: "every entry of one ledger append names the same scope".into(),
            });
        }
        let mut ledgers = self.ledgers()?;
        let index = match ledgers.iter().position(|(ledger, _)| ledger.scope == scope) {
            Some(index) => index,
            None => {
                let ledger = Ledger::new(scope, captured_at);
                let text = ledger.render()?;
                ledgers.push((ledger, text));
                ledgers.len() - 1
            }
        };
        let path = ledgers[index].0.path.clone();
        let mut text = ledgers[index].1.clone();
        let mut ids = Vec::with_capacity(decisions.len());
        for decision in decisions {
            let mut decision = decision.clone();
            decision.id = Ledger::parse(&path, &text)?.next_id();
            decision.validate()?;
            if let Some(target) = &decision.reverses
                && !ledgers
                    .iter()
                    .any(|(ledger, _)| ledger.entry(target).is_some())
                && !ids.contains(target)
            {
                return Err(office_core::Error::Library {
                    code: "decision_not_found",
                    message: format!("reverses names {target}, which no decision ledger holds"),
                });
            }
            text = append_line(&text, &decision.line()?);
            ids.push(decision.id);
        }
        let parsed = Ledger::parse(&path, &text)?;
        let pending = self.decide(WriteKind::Document, &path, &text, None, None)?;
        self.finish(&pending)?;
        ledgers[index] = (parsed, text);
        let parsed: Vec<Ledger> = ledgers.into_iter().map(|(l, _)| l).collect();
        let title = parsed[index].title();
        let all = views(&parsed);
        ids.iter()
            .map(|id| {
                let entry = all
                    .iter()
                    .find(|view| &view.decision.id == id)
                    .cloned()
                    .ok_or(office_core::Error::LibraryUnavailable)?;
                Ok(DecisionOutcome {
                    entry,
                    path: path.clone(),
                    title: title.clone(),
                })
            })
            .collect()
    }
}
