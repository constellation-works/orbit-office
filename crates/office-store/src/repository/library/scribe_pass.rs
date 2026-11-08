//! Recovery of a scribe pass journal (version 11) written by an earlier
//! build: the SCR file and the ledger and preference page files it names,
//! committed once. This build no longer starts a pass (the scribe moved to
//! the standalone scribe plugin, 4_decisions.md), but an interrupted write
//! still completes so no Library data is lost. Also the decision ledger
//! reads.

use std::path::Path;

use office_core::domain::artifact::ArtifactFile;
use office_core::domain::source::decisions::is_ledger_path;
use office_core::ports::LibraryText;

use super::LibraryStore;
use super::pending::PendingWrite;
use crate::{Error, Result};
use office_core::domain::knowledge::DocumentRecord;

/// Where an earlier build's scribe pass wrote preference pages.
const PREFERENCE_DIR: &str = "knowledge/domain";

/// Whether a pass may write `path`: a decision ledger or a preference page.
fn pass_file(path: &str) -> bool {
    is_ledger_path(Path::new(path))
        || path
            .strip_prefix(PREFERENCE_DIR)
            .and_then(|rest| rest.strip_prefix("/dom-preferences-"))
            .and_then(|rest| rest.strip_suffix(".md"))
            .is_some_and(|slug| {
                !slug.is_empty()
                    && slug
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
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
