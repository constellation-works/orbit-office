//! Library facade: files, rebuildable index, writer lock and git commit.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use office_common::fs::{atomic::write_atomic, hash::sha256_file};
use office_core::domain::settings::Settings;
use serde::{Deserialize, Serialize};

use office_core::domain::knowledge::DocumentKind;
use office_core::domain::template::{KnowledgePage, TemplateFile, assess_templates};
use office_core::ports::{InitOutcome, LibraryHealth};

use crate::contracts::{DocumentRecord, Revision, SearchFilter, SearchHit};
use crate::driver::file::library::LAYOUT_DIRS;
use crate::driver::file::{library::LibraryFs, lock::WriterLock};
use crate::driver::git::Git;
use crate::driver::sqlite::connection::IndexStore;
use crate::{Error, Result};

/// A Library and its index. Construction may create layout/index directories;
/// read methods never create state or wait for the writer lock.
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
    kind: WriteKind,
    root: PathBuf,
    relative: String,
    text: String,
    token: String,
}

/// A journal as read from disk, before its version gate is applied.
/// Version-1 journals predate `kind` and are always documents; version-2
/// journals must carry `kind` explicitly. Defaulting a missing `kind` on
/// a version-2 journal would silently change what the journal means, so
/// recovery refuses it instead (STD-02: a persisted shape only widens
/// through an explicit version step).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPendingWrite {
    version: u32,
    #[serde(default)]
    kind: Option<WriteKind>,
    root: PathBuf,
    relative: String,
    text: String,
    token: String,
}

impl RawPendingWrite {
    /// Apply the version gate and produce the replayable decision.
    fn into_pending(self, path: &Path) -> Result<PendingWrite> {
        let kind = match (self.version, self.kind) {
            (1, None) | (1, Some(WriteKind::Document)) => WriteKind::Document,
            (2, Some(kind)) => kind,
            _ => {
                return Err(Error::Journal {
                    path: path.to_path_buf(),
                });
            }
        };
        Ok(PendingWrite {
            version: self.version,
            kind,
            root: self.root,
            relative: self.relative,
            text: self.text,
            token: self.token,
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WriteKind {
    #[default]
    Document,
    Settings,
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
        let store = Self::from_parts(root, state, index)?;
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

    // Construction only: callers must recover under the writer lock before
    // exposing the store or performing another write.
    fn from_parts(root: PathBuf, state: &Path, index: IndexStore) -> Result<Self> {
        let state = state
            .canonicalize()
            .map_err(|e| Error::io("resolve plugin state", state, e))?;
        let git = Git::new(&root, &state);
        Ok(Self {
            files: LibraryFs::new(root),
            index,
            state,
            git,
        })
    }

    /// Scaffold a Library at the requested location (specs/library.md,
    /// "Scaffolding: `office.init`"), in spec order. Creates only what is
    /// missing and never overwrites an existing config, `.gitignore`,
    /// template or page. A complete Library, including every bundled
    /// template file, reports `already_initialized` with no commit.
    /// Holds the writer lock from before inspecting Library state through
    /// scaffolding, recovery, indexing and the initialization commit.
    pub fn initialize_at(
        library_root: &Path,
        state_dir: &Path,
        requested: Option<&str>,
        workspace_root: &Path,
    ) -> Result<InitOutcome> {
        let allowed = normalize_path(&workspace_root.join("library"));
        let candidate = match requested {
            Some(dir) if !dir.trim().is_empty() => {
                let path = Path::new(dir);
                normalize_path(&if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    workspace_root.join(path)
                })
            }
            _ => normalize_path(library_root),
        };
        if candidate != allowed && !candidate.starts_with(&allowed) {
            return Err(Error::LibraryDirOutside {
                requested: candidate,
                allowed,
            });
        }
        // Refuse a missing git before creating anything.
        let probe_cwd = if workspace_root.exists() {
            workspace_root.to_path_buf()
        } else {
            PathBuf::from("/")
        };
        Git::ensure_available(&probe_cwd)?;

        // Bootstrap only the lock directory; Library and index writes wait
        // until this guard is held, and completeness is checked under it.
        std::fs::create_dir_all(state_dir)
            .map_err(|e| Error::io("create writer state", state_dir, e))?;
        let lock = WriterLock::acquire(state_dir)?;
        let files = LibraryFs::new(candidate.clone());
        let git = Git::new(&candidate, state_dir);
        // Completeness is checked without creating layout or index.
        let is_repo = candidate.exists() && git.is_repository()?;
        let config_text = if candidate.exists() {
            files.settings_text()?
        } else {
            None
        };
        if let Some(text) = &config_text {
            // An invalid existing config is refused, never overwritten.
            Settings::from_yaml(text)?;
        }
        let gitignore_exists = candidate.join(".gitignore").exists();
        let index_exists = state_dir.join("index.sqlite").exists();
        let layout_complete = LAYOUT_DIRS.iter().all(|dir| {
            let path = candidate.join(dir);
            if *dir == ".office" {
                path.is_dir()
            } else {
                path.is_dir() && (path.join(".gitkeep").exists() || !dir_is_empty(&path))
            }
        });
        let templates_present = files.bundled_templates_present()?;
        if is_repo
            && config_text.is_some()
            && gitignore_exists
            && index_exists
            && layout_complete
            && templates_present
        {
            return Ok(InitOutcome {
                already_initialized: true,
                created: Vec::new(),
            });
        }

        let mut created: Vec<String> = Vec::new();
        for dir in LAYOUT_DIRS {
            if !candidate.join(dir).exists() {
                created.push((*dir).to_string());
            }
        }
        files.ensure_layout()?;
        for dir in LAYOUT_DIRS {
            if *dir == ".office" {
                continue;
            }
            let dir_path = candidate.join(dir);
            let keep = dir_path.join(".gitkeep");
            if dir_is_empty(&dir_path) && !keep.exists() {
                write_atomic(&keep, b"")?;
                created.push(format!("{dir}/.gitkeep"));
            }
        }
        if !is_repo {
            git.init()?;
            created.push(".git".to_string());
        }
        if config_text.is_none() {
            write_atomic(
                &candidate.join(".office/config.yaml"),
                Settings::default_yaml().as_bytes(),
            )?;
            created.push(".office/config.yaml".to_string());
        }
        if !gitignore_exists {
            write_atomic(&candidate.join(".gitignore"), b"original.*\n")?;
            created.push(".gitignore".to_string());
        }
        // Only the paths this call created are committed, so a later restore
        // does not stage an edit the user made to another template.
        created.extend(files.install_missing_templates()?);
        let root = candidate
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", &candidate, e))?;
        let index = IndexStore::open(&state_dir.join("index.sqlite"))?;
        let store = Self::from_parts(root, state_dir, index)?;
        if !index_exists {
            created.push("index.sqlite".to_string());
        }
        store.reindex_locked(&lock)?;
        let committable: Vec<String> = created
            .iter()
            .filter(|path| path.as_str() != ".git" && path.as_str() != "index.sqlite")
            .cloned()
            .collect();
        store
            .git
            .commit_created(&committable, "Initialize the Library")?;
        created.sort();
        Ok(InitOutcome {
            already_initialized: false,
            created,
        })
    }

    /// Library and index health. Counts come from the index; an
    /// uninitialized Library reports zero counts without reading it.
    pub fn health(&self) -> Result<LibraryHealth> {
        if !self.is_initialized()? {
            return Ok(LibraryHealth::uninitialized());
        }
        self.read_ready()?;
        let mut health = LibraryHealth {
            initialized: true,
            ..LibraryHealth::default()
        };
        for (kind, count) in self.index.counts_by_kind()? {
            match DocumentKind::parse(&kind) {
                Some(DocumentKind::Source) => health.source = count,
                Some(DocumentKind::Domain) => health.domain = count,
                Some(DocumentKind::Runbook) => health.runbook = count,
                Some(DocumentKind::Artifact) => health.artifact = count,
                None => {}
            }
            health.documents_total = health.documents_total.saturating_add(count);
        }
        let templates = self.files.read_templates()?;
        let template_files: Vec<TemplateFile<'_>> = templates
            .iter()
            .map(|template| TemplateFile {
                path: &template.relative,
                markdown: template.markdown.as_deref(),
            })
            .collect();
        let knowledge = self.files.read_knowledge_markdown()?;
        let pages: Vec<KnowledgePage<'_>> = knowledge
            .iter()
            .filter_map(|(relative, text)| {
                let record = DocumentRecord::from_markdown(Path::new(relative), text);
                match record.kind {
                    DocumentKind::Domain | DocumentKind::Runbook => Some(KnowledgePage {
                        kind: record.kind,
                        markdown: text.as_str(),
                    }),
                    DocumentKind::Source | DocumentKind::Artifact => None,
                }
            })
            .collect();
        let assessment = assess_templates(&template_files, &pages);
        health.pages_off_template = assessment.pages_off_template;
        health.template_invalid = assessment.template_invalid;
        Ok(health)
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

    /// Apply exactly one typed setting to the latest config while holding
    /// the document writer lock. A settings decision skips the document index.
    pub fn set_setting(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let settings = self.settings()?.with_setting(key, value)?;
        let text = settings.to_yaml()?;
        self.files.validate_settings_path()?;
        let pending = self.decide(WriteKind::Settings, ".office/config.yaml", &text)?;
        self.finish(&pending)
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
            && WriterLock::journal_is_abandoned(&self.state, &self.journal_path())?
        {
            return Err(Error::PendingWrite {
                root: self.library_root().into(),
            });
        }
        self.require_initialized()
    }

    /// Rebuild the index under the same writer lock used by document writes.
    pub fn reindex(&self) -> Result<u32> {
        let lock = WriterLock::acquire(&self.state)?;
        self.reindex_locked(&lock)
    }

    fn reindex_locked(&self, _lock: &WriterLock) -> Result<u32> {
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
    /// Only a path `reindex` scans is accepted. Any other Markdown path is
    /// refused before the journal, the file, the index or the commit.
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
        let pending = self.decide(WriteKind::Document, relative, text)?;
        self.finish(&pending)?;
        self.files.read_document(Path::new(relative))
    }

    fn decide(&self, kind: WriteKind, relative: &str, text: &str) -> Result<PendingWrite> {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| {
            Error::io(
                "read write decision time",
                self.library_root(),
                std::io::Error::other(e),
            )
        })?;
        let pending = PendingWrite {
            version: 2,
            kind,
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
        Ok(pending)
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
        let raw: RawPendingWrite =
            serde_json::from_slice(&bytes).map_err(|_| Error::Journal { path: path.clone() })?;
        let pending = raw.into_pending(&path)?;
        if !(1..=2).contains(&pending.version)
            || pending.root != self.library_root()
            || pending.token.is_empty()
            || !pending
                .token
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::Journal { path });
        }
        match pending.kind {
            WriteKind::Document => {
                self.require_initialized()?;
                self.files
                    .validate_document_path(Path::new(&pending.relative))?;
                self.validate_identity(&DocumentRecord::from_markdown(
                    Path::new(&pending.relative),
                    &pending.text,
                ))?;
            }
            WriteKind::Settings => {
                if pending.version != 2 || pending.relative != ".office/config.yaml" {
                    return Err(Error::Journal { path });
                }
                // A decided config replacement is recoverable even if its
                // old file is missing or invalid; validate the decided text.
                if !self.git.is_repository()? {
                    return Err(Error::NotInitialized {
                        root: self.library_root().into(),
                    });
                }
                self.files.validate_settings_path()?;
                Settings::from_yaml(&pending.text)?;
            }
        }
        self.finish(&pending)
    }

    fn finish(&self, pending: &PendingWrite) -> Result<()> {
        match pending.kind {
            WriteKind::Document => {
                let relative = Path::new(&pending.relative);
                self.files.write_document(relative, &pending.text)?;
                let record = self.files.read_document(relative)?;
                self.index.upsert(&record)?;
            }
            WriteKind::Settings => self.files.write_settings(&pending.text)?,
        }
        self.git.commit(&pending.relative, &pending.token)?;
        let path = self.journal_path();
        std::fs::remove_file(&path)
            .map_err(|e| Error::io("clear completed write journal", path, e))?;
        // Persist removal, so a crash cannot resurrect a completed obligation.
        std::fs::File::open(&self.state)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| Error::io("sync writer state", &self.state, e))?;
        Ok(())
    }

    /// Read a current document from disk, without waiting for the writer lock.
    pub fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        self.read_ready()?;
        self.files.validate_document_path(relative)?;
        self.files.read_document(relative)
    }

    /// One indexed document by id, if present. Does not wait for the writer lock.
    pub fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        self.read_ready()?;
        self.index.get(id)
    }

    /// Number of indexed documents. Does not wait for the writer lock.
    pub fn document_count(&self) -> Result<u32> {
        self.read_ready()?;
        self.index.count()
    }

    /// Full-text search; filters apply before the limit. No writer-lock wait.
    pub fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        self.read_ready()?;
        self.index.search(query, filter)
    }

    /// Newest-first commits touching this document. No writer-lock wait.
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

/// Lexically resolve `.` and `..` without requiring the path to exist.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            std::path::Component::RootDir => out.push(std::path::Component::RootDir.as_os_str()),
            std::path::Component::Normal(name) => out.push(name),
        }
    }
    out
}

fn dir_is_empty(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
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
    fn set_setting(&self, key: &str, value: serde_json::Value) -> office_core::Result<()> {
        Ok(Self::set_setting(self, key, value)?)
    }
    fn initialize(&self, library_dir: Option<&str>) -> office_core::Result<InitOutcome> {
        let workspace = self
            .library_root()
            .parent()
            .ok_or(office_core::Error::LibraryUnavailable)?;
        Ok(Self::initialize_at(
            self.library_root(),
            &self.state,
            library_dir,
            workspace,
        )?)
    }
    fn reindex(&self) -> office_core::Result<u32> {
        if !Self::is_initialized(self)? {
            return Err(office_core::ports::not_initialized_error());
        }
        Ok(Self::reindex(self)?)
    }
    fn health(&self) -> office_core::Result<LibraryHealth> {
        Ok(Self::health(self)?)
    }
}
