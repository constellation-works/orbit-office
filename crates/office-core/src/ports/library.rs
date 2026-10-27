//! Library access port. Defaults refuse access for an unconfigured context.

use std::path::Path;

use crate::domain::knowledge::{DocumentRecord, Revision, SearchFilter, SearchHit};
use crate::domain::settings::Settings;
use crate::{Error, Result};

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
}
