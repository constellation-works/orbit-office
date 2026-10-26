//! The Library facade: one Library root (files, the source of truth)
//! plus one index database under plugin state (rebuildable).

use std::path::Path;

use crate::contracts::{DocumentRecord, SearchFilter, SearchHit};
use crate::driver::file::library::LibraryFs;
use crate::driver::sqlite::connection::IndexStore;
use crate::error::Result;
use office_common::fs::hash::sha256_file;

/// A Library and its index, opened together.
pub struct LibraryStore {
    files: LibraryFs,
    index: IndexStore,
}

impl LibraryStore {
    /// Open the Library at `library_root` with its index at
    /// `<state_dir>/index.sqlite`, creating the layout and the index if
    /// needed and migrating the index to the supported schema.
    pub fn open(library_root: &Path, state_dir: &Path) -> Result<Self> {
        let files = LibraryFs::new(library_root.to_path_buf());
        files.ensure_layout()?;
        let index = IndexStore::open(&state_dir.join("index.sqlite"))?;
        Ok(Self { files, index })
    }

    /// Open with an in-memory index (tests and one-shot scans).
    pub fn open_in_memory(library_root: &Path) -> Result<Self> {
        let files = LibraryFs::new(library_root.to_path_buf());
        files.ensure_layout()?;
        let index = IndexStore::open_in_memory()?;
        Ok(Self { files, index })
    }

    /// The Library root (plain files; the source of truth).
    #[must_use]
    pub fn library_root(&self) -> &Path {
        self.files.root()
    }

    /// The index database file.
    #[must_use]
    pub fn index_path(&self) -> &Path {
        self.index.path()
    }

    /// Rebuild the index from the files on disk (`office.reindex`).
    /// Returns the number of documents indexed.
    pub fn reindex(&self) -> Result<u32> {
        let documents = self.files.scan_documents()?;
        let count = u32::try_from(documents.len()).unwrap_or(u32::MAX);
        self.index.rebuild(&documents)?;
        Ok(count)
    }

    /// Write one document (atomically) and index it in the same call, so
    /// the index never lags a successful write.
    pub fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        self.files.write_document(relative, text)?;
        let record = self.files.read_document(relative)?;
        self.index.upsert(&record)?;
        Ok(record)
    }

    /// Read one document from disk by Library-relative path.
    pub fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        self.files.read_document(relative)
    }

    /// One indexed document by id, if present.
    pub fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        self.index.get(id)
    }

    /// Number of indexed documents.
    pub fn document_count(&self) -> Result<u32> {
        self.index.count()
    }

    /// Full-text search over the index. Filters apply before the limit.
    pub fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        self.index.search(query, filter)
    }

    /// Hex SHA-256 of a Library file (capture dedupe and `original.*`
    /// pinning both key on this).
    pub fn file_sha256(&self, relative: &Path) -> Result<String> {
        Ok(sha256_file(&self.library_root().join(relative))?)
    }
}
