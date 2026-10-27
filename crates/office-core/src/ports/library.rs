//! Library access port. Defaults refuse access for an unconfigured context.

use std::path::Path;

use crate::domain::knowledge::{
    Backlink, DocumentKind, DocumentRecord, Revision, SearchFilter, SearchHit,
};
use crate::domain::settings::Settings;
use crate::{Error, Result};

/// What `office.init` created, or that it found a complete Library.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitOutcome {
    /// True when a complete Library already existed and nothing changed.
    pub already_initialized: bool,
    /// Library-relative (or state-relative) paths this call created, sorted.
    pub created: Vec<String>,
}

/// One Markdown file read from the Library, by its Library-relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryText {
    /// Library-relative path, forward slashes.
    pub path: String,
    /// The file's Markdown. `None` when it could not be read (a symlink or
    /// an unreadable template), so a caller can name it rather than skip it.
    pub markdown: Option<String>,
}

/// Library and index health for `office.status`. Counts come from the index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryHealth {
    /// The Library's own repository and a valid settings file are present.
    pub initialized: bool,
    /// Indexed documents of every kind.
    pub documents_total: u32,
    /// Indexed `source` documents.
    pub source: u32,
    /// Indexed `domain` pages.
    pub domain: u32,
    /// Indexed `runbook` pages.
    pub runbook: u32,
    /// Indexed `artifact` documents.
    pub artifact: u32,
    /// Knowledge pages that name a template and no longer meet its current rules.
    pub pages_off_template: u32,
    /// Template files that do not parse, as Library-relative paths, sorted.
    /// Empty when every template file is valid.
    pub template_invalid: Vec<String>,
}

impl LibraryHealth {
    /// Health for a Library that is not initialized: no counts are read.
    #[must_use]
    pub fn uninitialized() -> Self {
        Self::default()
    }

    /// Count for one kind.
    #[must_use]
    pub fn count_for(&self, kind: DocumentKind) -> u32 {
        match kind {
            DocumentKind::Source => self.source,
            DocumentKind::Domain => self.domain,
            DocumentKind::Runbook => self.runbook,
            DocumentKind::Artifact => self.artifact,
        }
    }
}

/// The shared refusal for an uninitialized Library (STD-02 R24): one
/// definition, used by the dispatch chokepoint and by adapters.
#[must_use]
pub fn not_initialized_error() -> Error {
    Error::Library {
        code: "library_not_initialized",
        message: "Library is not initialized; run office.init".to_string(),
    }
}

/// The Library operations used by tools, independent of files, git and SQLite.
/// An adapter must override all operations; defaults support contexts that have
/// no Library (for example the status-only dispatch scaffold).
pub trait Library {
    /// Read the current document by Library-relative path, without a writer lock.
    fn read_document(&self, _relative: &Path) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// Write a document, update its index and commit under one writer lock.
    fn write_document(&self, _relative: &Path, _text: &str) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// One indexed document by id (with its body, cites and related), if
    /// present. Does not wait for the writer lock.
    fn get(&self, _id: &str) -> Result<Option<DocumentRecord>> {
        Err(Error::LibraryUnavailable)
    }
    /// Indexed documents whose frontmatter `cites` (`<id>` or
    /// `<id>#<anchor>`) or `related` names `id`, sorted by id.
    fn backlinks(&self, _id: &str) -> Result<Vec<Backlink>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every knowledge page file (`knowledge/**/*.md`), in path order.
    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every template file in `.office/templates/`, in path order.
    fn templates(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Search the index, applying filters before the limit.
    fn search(&self, _query: &str, _filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        Err(Error::LibraryUnavailable)
    }
    /// Newest-first commits affecting a document (empty for an untracked path).
    fn history(&self, _relative: &Path) -> Result<Vec<Revision>> {
        Err(Error::LibraryUnavailable)
    }
    /// Check for the Library's own repository and settings file, without creating them.
    fn is_initialized(&self) -> Result<bool> {
        Err(Error::LibraryUnavailable)
    }
    /// Load and validate settings on every call.
    fn settings(&self) -> Result<Settings> {
        Err(Error::LibraryUnavailable)
    }
    /// Apply one typed setting to the latest config and commit under one
    /// writer lock. Unknown keys and invalid values leave it unchanged.
    fn set_setting(&self, _key: &str, _value: serde_json::Value) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Scaffold the Library (layout, git, config, `.gitignore`, index, one
    /// commit). `library_dir`, when given, is the requested Library location
    /// (workspace-relative or absolute); an adapter refuses one outside its
    /// granted `{{workspace}}/library` root with `library_dir_outside_workspace`
    /// and a missing `git` with `git_missing`. Idempotent.
    fn initialize(&self, _library_dir: Option<&str>) -> Result<InitOutcome> {
        Err(Error::LibraryUnavailable)
    }
    /// Rebuild the index from the files; returns the indexed document count.
    fn reindex(&self) -> Result<u32> {
        Err(Error::LibraryUnavailable)
    }
    /// Library and index health, without creating Library state.
    fn health(&self) -> Result<LibraryHealth> {
        Ok(LibraryHealth {
            initialized: self.is_initialized()?,
            ..LibraryHealth::default()
        })
    }
}
