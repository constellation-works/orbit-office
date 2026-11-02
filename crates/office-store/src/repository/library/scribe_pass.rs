//! Scribe v2 passes in the Library store (specs/scribe.md, "Pipeline"
//! step 4): the SCR file and the ledger and preference page files a pass
//! plans, compare-and-set under the writer lock and committed once
//! (journal version 11).

use std::path::Path;

use office_core::domain::artifact::ArtifactFile;
use office_core::domain::source::conversation::scribe::v2;
use office_core::domain::source::decisions::is_ledger_path;
use office_core::ports::{LibraryText, ScribePass};

use super::LibraryStore;
use super::pending::{PendingWrite, WriteKind};
use crate::contracts::DocumentRecord;
use crate::driver::file::lock::WriterLock;
use crate::{Error, Result};

/// Whether a pass may write `path`: a decision ledger or a preference page.
fn pass_file(path: &str) -> bool {
    is_ledger_path(Path::new(path))
        || path
            .strip_prefix(v2::PREFERENCE_DIR)
            .and_then(|rest| rest.strip_prefix("/dom-preferences-"))
            .and_then(|rest| rest.strip_suffix(".md"))
            .is_some_and(|slug| {
                !slug.is_empty()
                    && slug
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
}

fn changed() -> office_core::Error {
    office_core::domain::source::video::refusal(
        "scribe_changed",
        "the SCR, a ledger or a preference page changed since the pass was planned; retry",
    )
}

impl LibraryStore {
    /// Every decision ledger's path and Markdown. No writer-lock wait.
    pub fn decision_ledger_files(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        self.require_initialized()?;
        Ok(self
            .files
            .read_ledger_markdown()?
            .into_iter()
            .map(|(path, markdown)| LibraryText {
                path,
                markdown: Some(markdown),
            })
            .collect())
    }

    pub(super) fn current_text(&self, relative: &Path) -> Result<Option<String>> {
        match self.files.read_text(relative) {
            Ok(text) => Ok(Some(text)),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Write one v2 pass and commit it, under the writer lock.
    pub fn write_scribe_pass(&self, pass: &ScribePass) -> office_core::Result<()> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let target = Path::new(&pass.scr.path);
        self.files.validate_scribe_path(target)?;
        let planned = match &pass.scr.previous {
            Some((path, text)) if path == &pass.scr.path => Some(text.as_str()),
            Some(_) => {
                return Err(office_core::Error::InvalidArgument(
                    "a v2 SCR is never renamed".into(),
                ));
            }
            None => None,
        };
        if self.files.read_scribe(target)?.as_deref() != planned {
            return Err(changed());
        }
        if let Some(chain) = &pass.chain {
            let files = self.scribe_files()?;
            let scrs = v2::v2_files(&files)?;
            let latest = v2::latest_in_scope(&scrs, &chain.scope, &chain.session)
                .map(|scr| scr.front.id.clone());
            if latest != chain.previous {
                return Err(changed());
            }
        }
        let mut bundle = Vec::new();
        for file in &pass.files {
            let relative = Path::new(&file.path);
            if !pass_file(&file.path) {
                return Err(office_core::Error::InvalidArgument(
                    "a scribe pass writes only decision ledgers and preference pages".into(),
                ));
            }
            self.files.validate_document_path(relative)?;
            if self.current_text(relative)? != file.expected {
                return Err(changed());
            }
            bundle.push(ArtifactFile {
                path: file.path.clone(),
                bytes: file.text.clone().into_bytes(),
            });
        }
        let pending = self.decide(
            WriteKind::ScribePass,
            &pass.scr.path,
            &pass.scr.text,
            None,
            Some(bundle),
        )?;
        Ok(self.finish(&pending)?)
    }

    /// A version-11 journal names one SCR and only ledger and preference
    /// page files, each once, each a readable document with a stable id.
    pub(super) fn validate_scribe_pass_bundle(
        &self,
        pending: &PendingWrite,
        files: &[ArtifactFile],
    ) -> Result<()> {
        let journal = || Error::Journal {
            path: self.journal_path(),
        };
        if pending.version != 11 || pending.original.is_some() || pending.removed.is_some() {
            return Err(journal());
        }
        self.files
            .validate_scribe_path(Path::new(&pending.relative))?;
        let mut seen = std::collections::BTreeSet::new();
        for file in files {
            if !pass_file(&file.path) || !seen.insert(&file.path) {
                return Err(journal());
            }
            let relative = Path::new(&file.path);
            self.files.validate_document_path(relative)?;
            let markdown = std::str::from_utf8(&file.bytes).map_err(|_| journal())?;
            self.validate_identity(&DocumentRecord::from_markdown(relative, markdown))?;
        }
        Ok(())
    }
}
