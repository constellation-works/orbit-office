//! Decision ledgers in the Library store (specs/decisions.md, Decision
//! ledger): resolving ledger, entry and migrated note ids, appending one
//! entry under the writer lock, and writing a planned batch of ledger files
//! once (Orbit decision ingestion, journal version 12).

use std::collections::BTreeSet;
use std::path::Path;

use office_core::domain::artifact::ArtifactFile;
use office_core::domain::source::decisions::PlannedFile;
use office_core::domain::source::decisions::is_ledger_path;

use office_core::domain::source::decisions::{
    Decision, Ledger, Resolved, append_line, entry_record, parse_entry_id, resolve, same_as_match,
    views,
};
use office_core::domain::source::source_body;
use office_core::ports::{DecisionOutcome, SourceView};

use super::LibraryStore;
use super::pending::{PendingWrite, WriteKind};
use crate::driver::file::lock::WriterLock;
use crate::{Error, Result};
use office_core::domain::knowledge::DocumentRecord;

impl LibraryStore {
    /// Provider-only tolerant scan; other ledger readers remain strict.
    pub fn decision_precedent_ledgers(&self) -> Result<Vec<office_core::ports::LibraryText>> {
        self.require_initialized()?;
        self.files.read_precedent_ledgers()
    }
    /// Rank an eligible decision snapshot with the Library index's FTS5
    /// tokenizer and BM25. Ledger lines are not persistent index documents;
    /// this private in-memory index writes no durable state on a read.
    pub(super) fn decision_precedent_scores(
        &self,
        query: &str,
        entries: &[(String, String)],
    ) -> Result<std::collections::BTreeMap<String, f64>> {
        let path = Path::new("<in-memory>");
        let error = |source| Error::sqlite(path, "rank decision precedents", source);
        let terms: BTreeSet<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|term| !term.is_empty())
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect();
        if terms.is_empty() || entries.is_empty() {
            return Ok(Default::default());
        }
        let conn = rusqlite::Connection::open_in_memory().map_err(error)?;
        conn.execute_batch("PRAGMA temp_store=MEMORY; CREATE VIRTUAL TABLE decisions_fts USING fts5(id UNINDEXED, body);").map_err(error)?;
        {
            let mut insert = conn
                .prepare("INSERT INTO decisions_fts (id,body) VALUES (?1,?2)")
                .map_err(error)?;
            for (id, text) in entries {
                insert.execute(rusqlite::params![id, text]).map_err(error)?;
            }
        }
        let mut stmt = conn
            .prepare(
                "SELECT id, -bm25(decisions_fts) FROM decisions_fts WHERE decisions_fts MATCH ?1",
            )
            .map_err(error)?;
        let query = terms.into_iter().collect::<Vec<_>>().join(" OR ");
        let ranks = stmt
            .query_map([query], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(error)?;
        let mut scores = std::collections::BTreeMap::new();
        let mut best = 0.0_f64;
        for rank in ranks {
            let (id, score) = rank.map_err(error)?;
            best = best.max(score);
            scores.insert(id, score);
        }
        if best > 0.0 {
            for score in scores.values_mut() {
                *score /= best;
            }
        }
        Ok(scores)
    }

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
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let scope = decision.parsed_scope()?;
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
        let mut decision = decision.clone();
        decision.id = ledgers[index].0.next_id();
        decision.validate()?;
        if let Some(target) = &decision.reverses
            && !ledgers
                .iter()
                .any(|(ledger, _)| ledger.entry(target).is_some())
        {
            return Err(office_core::Error::Library {
                code: "decision_not_found",
                message: format!("reverses names {target}, which no decision ledger holds"),
            });
        }
        if let Some(target) = &decision.same_as
            && !ledgers
                .iter()
                .any(|(ledger, _)| ledger.entry(target).is_some())
        {
            return Err(office_core::Error::Library {
                code: "decision_not_found",
                message: format!("same_as names {target}, which no decision ledger holds"),
            });
        }
        if decision.same_as.is_none() {
            let existing: Vec<Decision> = ledgers
                .iter()
                .flat_map(|(ledger, _)| ledger.entries.iter().map(|e| e.decision.clone()))
                .collect();
            decision.same_as = same_as_match(&decision, existing.iter());
        }
        let path = ledgers[index].0.path.clone();
        let text = append_line(&ledgers[index].1, &decision.line()?);
        let parsed = Ledger::parse(&path, &text)?;
        let pending = self.decide(WriteKind::Document, &path, &text, None, None)?;
        self.finish(&pending)?;
        ledgers[index] = (parsed, text);
        let parsed: Vec<Ledger> = ledgers.into_iter().map(|(l, _)| l).collect();
        let title = parsed[index].title();
        let entry = views(&parsed)
            .into_iter()
            .find(|view| view.decision.id == decision.id)
            .ok_or(office_core::Error::LibraryUnavailable)?;
        Ok(DecisionOutcome { entry, path, title })
    }

    /// Write planned ledger files and commit them once, under the writer
    /// lock. Each file is compared against the text it was planned against
    /// (`expected`, `None` for a new ledger) and refused, writing nothing,
    /// with `decision_ledger_changed` when it differs; every file must be a
    /// ledger path, named once, that parses as a ledger. No files: no write.
    pub fn write_decision_ledgers(&self, files: &[PlannedFile]) -> office_core::Result<()> {
        let Some((first, rest)) = files.split_first() else {
            return Ok(());
        };
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let mut seen = BTreeSet::new();
        for file in files {
            let relative = Path::new(&file.path);
            if !is_ledger_path(relative) || !seen.insert(file.path.as_str()) {
                return Err(office_core::Error::InvalidArgument(
                    "a decision ledger batch writes only decision ledgers, each once".into(),
                ));
            }
            self.files.validate_document_path(relative)?;
            if self.current_text(relative)? != file.expected {
                return Err(office_core::Error::Library {
                    code: "decision_ledger_changed",
                    message: "a decision ledger changed since the batch was planned; retry".into(),
                });
            }
            Ledger::parse(&file.path, &file.text)?;
        }
        let bundle = rest
            .iter()
            .map(|file| ArtifactFile {
                path: file.path.clone(),
                bytes: file.text.clone().into_bytes(),
            })
            .collect();
        let pending = self.decide(
            WriteKind::DecisionLedgers,
            &first.path,
            &first.text,
            None,
            Some(bundle),
        )?;
        Ok(self.finish(&pending)?)
    }

    /// A version-12 journal names only ledger files, each once, each a
    /// parseable ledger.
    pub(super) fn validate_decision_ledgers_bundle(
        &self,
        pending: &PendingWrite,
        files: &[ArtifactFile],
    ) -> Result<()> {
        let journal = || Error::Journal {
            path: self.journal_path(),
        };
        if pending.version != 12 || pending.original.is_some() || pending.removed.is_some() {
            return Err(journal());
        }
        let mut seen = BTreeSet::new();
        let first = (pending.relative.as_str(), pending.text.as_bytes());
        for (path, bytes) in std::iter::once(first).chain(
            files
                .iter()
                .map(|file| (file.path.as_str(), file.bytes.as_slice())),
        ) {
            let relative = Path::new(path);
            if !is_ledger_path(relative) || !seen.insert(path) {
                return Err(journal());
            }
            self.files.validate_document_path(relative)?;
            let markdown = std::str::from_utf8(bytes).map_err(|_| journal())?;
            Ledger::parse(path, markdown).map_err(|_| journal())?;
        }
        Ok(())
    }
}
