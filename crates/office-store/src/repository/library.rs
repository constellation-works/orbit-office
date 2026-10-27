//! Library facade: files, rebuildable index, writer lock and git commit.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use office_common::fs::{atomic::write_atomic, hash::sha256_file};
use office_core::domain::settings::Settings;
use serde::{Deserialize, Serialize};

use crate::contracts::{DocumentRecord, Revision, SearchFilter, SearchHit};
use crate::driver::file::{library::LibraryFs, lock::WriterLock};
use crate::driver::git::Git;
use crate::driver::sqlite::connection::IndexStore;
use crate::{Error, Result};

/// A Library and its index. Construction may create layout/index directories;
/// read methods never create state or acquire the writer lock.
pub struct LibraryStore {
    files: LibraryFs,
    index: IndexStore,
    state: PathBuf,
    git: Git,
}

/// A durable decision to finish one write. Replayed idempotently while locked.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingWrite {
    version: u32,
    root: PathBuf,
    relative: String,
    text: String,
    token: String,
}

impl LibraryStore {
    /// Open the Library and `<state_dir>/index.sqlite`, creating the layout
    /// and index as needed. Does not initialize git or create settings.
    /// An interrupted decided write is recovered before this returns.
    pub fn open(library_root: &Path, state_dir: &Path) -> Result<Self> {
        let files = LibraryFs::new(library_root.to_path_buf());
        files.ensure_layout()?;
        let root = library_root
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", library_root, e))?;
        let index = IndexStore::open(&state_dir.join("index.sqlite"))?;
        Self::compose(root, state_dir, index)
    }

    /// Open with a rebuildable in-memory index. Writer state remains on disk
    /// at `<library_root>/.office/state` so writes are still serialized.
    /// Use `open` with one shared plugin state for production calls.
    pub fn open_in_memory(library_root: &Path) -> Result<Self> {
        let files = LibraryFs::new(library_root.to_path_buf());
        files.ensure_layout()?;
        let root = library_root
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", library_root, e))?;
        let state = root.join(".office/state");
        std::fs::create_dir_all(&state).map_err(|e| Error::io("create writer state", &state, e))?;
        Self::compose(root, &state, IndexStore::open_in_memory()?)
    }

    fn compose(root: PathBuf, state: &Path, index: IndexStore) -> Result<Self> {
        let state = state
            .canonicalize()
            .map_err(|e| Error::io("resolve plugin state", state, e))?;
        let git = Git::new(&root, &state);
        let store = Self {
            files: LibraryFs::new(root),
            index,
            state,
            git,
        };
        if store
            .journal_path()
            .try_exists()
            .map_err(|e| Error::io("inspect write journal", store.journal_path(), e))?
        {
            let _lock = WriterLock::acquire(&store.state)?;
            store.recover()?;
        }
        Ok(store)
    }

    /// Resolved Library root (Markdown files are the source of truth).
    #[must_use]
    pub fn library_root(&self) -> &Path {
        self.files.root()
    }

    /// The index database file (or `<in-memory>`).
    #[must_use]
    pub fn index_path(&self) -> &Path {
        self.index.path()
    }

    /// True only for the Library's own repository and a valid settings file.
    /// Missing repository/config returns false; unreadable or invalid config fails.
    pub fn is_initialized(&self) -> Result<bool> {
        if !self.git.is_repository()? {
            return Ok(false);
        }
        match self.files.settings_text()? {
            Some(text) => {
                Settings::from_yaml(&text)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Read and validate the settings afresh. No cache masks hand-edited values.
    pub fn settings(&self) -> Result<Settings> {
        self.read_ready()?;
        let text = self
            .files
            .settings_text()?
            .ok_or_else(|| Error::NotInitialized {
                root: self.library_root().into(),
            })?;
        Ok(Settings::from_yaml(&text)?)
    }

    fn require_initialized(&self) -> Result<()> {
        if self.is_initialized()? {
            Ok(())
        } else {
            Err(Error::NotInitialized {
                root: self.library_root().into(),
            })
        }
    }

    fn read_ready(&self) -> Result<()> {
        if self
            .journal_path()
            .try_exists()
            .map_err(|e| Error::io("inspect write journal", self.journal_path(), e))?
        {
            return Err(Error::PendingWrite {
                root: self.library_root().into(),
            });
        }
        self.require_initialized()
    }

    /// Rebuild the index under the same writer lock used by document writes.
    pub fn reindex(&self) -> Result<u32> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let documents = self.files.scan_documents()?;
        let count = u32::try_from(documents.len()).map_err(|e| {
            Error::io(
                "count Library documents",
                self.library_root(),
                std::io::Error::other(e),
            )
        })?;
        self.index.rebuild(&documents)?;
        Ok(count)
    }

    /// Decide, write, index and commit a document under one OS writer lock.
    /// A failure after the durable journal is written leaves a recoverable
    /// obligation; opening the store or the next write completes it.
    pub fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        self.files.validate_document_path(relative)?;
        self.validate_identity(&DocumentRecord::from_markdown(relative, text))?;
        let relative = relative.to_str().ok_or_else(|| Error::UnsafePath {
            kind: "document",
            value: relative.display().to_string(),
        })?;
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| {
            Error::io(
                "read write decision time",
                self.library_root(),
                std::io::Error::other(e),
            )
        })?;
        let pending = PendingWrite {
            version: 1,
            root: self.library_root().into(),
            relative: relative.into(),
            text: text.into(),
            token: format!("{}-{}", std::process::id(), time.as_nanos()),
        };
        let bytes = serde_json::to_vec(&pending).map_err(|_| Error::Journal {
            path: self.journal_path(),
        })?;
        // One durable decision covers files, SQLite and git (STD-03 R8).
        write_atomic(&self.journal_path(), &bytes)?;
        self.finish(&pending)
    }

    fn validate_identity(&self, record: &DocumentRecord) -> Result<()> {
        if let Some(existing) = self.index.get(&record.id)?
            && existing.path != record.path
        {
            return Err(Error::DocumentConflict {
                id: record.id.clone(),
                existing_path: existing.path,
                path: record.path.clone(),
            });
        }
        Ok(())
    }

    fn journal_path(&self) -> PathBuf {
        self.state.join("library.pending-write.json")
    }

    fn recover(&self) -> Result<()> {
        let path = self.journal_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Error::io("read write journal", &path, e)),
        };
        let pending: PendingWrite =
            serde_json::from_slice(&bytes).map_err(|_| Error::Journal { path: path.clone() })?;
        if pending.version != 1
            || pending.root != self.library_root()
            || pending.token.is_empty()
            || !pending
                .token
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::Journal { path });
        }
        self.require_initialized()?;
        self.files
            .validate_document_path(Path::new(&pending.relative))?;
        self.validate_identity(&DocumentRecord::from_markdown(
            Path::new(&pending.relative),
            &pending.text,
        ))?;
        self.finish(&pending).map(|_| ())
    }

    fn finish(&self, pending: &PendingWrite) -> Result<DocumentRecord> {
        let relative = Path::new(&pending.relative);
        self.files.write_document(relative, &pending.text)?;
        let record = self.files.read_document(relative)?;
        self.index.upsert(&record)?;
        self.git.commit(&pending.relative, &pending.token)?;
        let path = self.journal_path();
        std::fs::remove_file(&path)
            .map_err(|e| Error::io("clear completed write journal", path, e))?;
        // Persist removal, so a crash cannot resurrect a completed obligation.
        std::fs::File::open(&self.state)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| Error::io("sync writer state", &self.state, e))?;
        Ok(record)
    }

    /// Read a current document from disk, without the writer lock.
    pub fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        self.read_ready()?;
        self.files.validate_document_path(relative)?;
        self.files.read_document(relative)
    }

    /// One indexed document by id, if present. Does not take the writer lock.
    pub fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        self.read_ready()?;
        self.index.get(id)
    }

    /// Number of indexed documents. Does not take the writer lock.
    pub fn document_count(&self) -> Result<u32> {
        self.read_ready()?;
        self.index.count()
    }

    /// Full-text search; filters apply before the limit. No writer lock.
    pub fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        self.read_ready()?;
        self.index.search(query, filter)
    }

    /// Newest-first commits touching this document. No writer lock.
    pub fn history(&self, relative: &Path) -> Result<Vec<Revision>> {
        self.read_ready()?;
        self.files.validate_document_path(relative)?;
        let relative = relative.to_str().ok_or_else(|| Error::UnsafePath {
            kind: "document",
            value: relative.display().to_string(),
        })?;
        self.git.history(relative)
    }

    /// SHA-256 of a Library file, including an `original.*` upload.
    pub fn file_sha256(&self, relative: &Path) -> Result<String> {
        self.read_ready()?;
        self.files.validate_relative_path(relative)?;
        Ok(sha256_file(&self.library_root().join(relative))?)
    }
}

impl office_core::ports::Library for LibraryStore {
    fn read_document(&self, relative: &Path) -> office_core::Result<DocumentRecord> {
        Ok(Self::read_document(self, relative)?)
    }
    fn write_document(&self, relative: &Path, text: &str) -> office_core::Result<DocumentRecord> {
        Ok(Self::write_document(self, relative, text)?)
    }
    fn search(&self, query: &str, filter: &SearchFilter) -> office_core::Result<Vec<SearchHit>> {
        Ok(Self::search(self, query, filter)?)
    }
    fn history(&self, relative: &Path) -> office_core::Result<Vec<Revision>> {
        Ok(Self::history(self, relative)?)
    }
    fn is_initialized(&self) -> office_core::Result<bool> {
        Ok(Self::is_initialized(self)?)
    }
    fn settings(&self) -> office_core::Result<Settings> {
        Ok(Self::settings(self)?)
    }
}
