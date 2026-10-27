//! Library facade: files, rebuildable index, writer lock and git commit.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use office_common::Sensitivity;
use office_common::fs::{atomic::write_atomic, hash::sha256_file};
use office_core::domain::artifact::{Artifact, ArtifactFile};
use office_core::domain::settings::Settings;
use office_core::domain::source::{
    RenderSource, SourceKind, format_conversation_entry, is_video_extension,
    media_type_for_extension, normalize_text, parse_source_meta, render_source, sha256_hex,
    source_body, source_id_base, unique_source_id, vtt_to_body,
};
use office_core::ports::{CaptureSourceRequest, SourceOutcome, SourceView};
use serde::{Deserialize, Serialize};

use office_core::domain::knowledge::DocumentKind;
use office_core::domain::template::{KnowledgePage, TemplateFile, assess_templates};
use office_core::ports::{InitOutcome, LibraryHealth, LibraryText};

use crate::contracts::{Backlink, DocumentRecord, Revision, SearchFilter, SearchHit};
use crate::driver::convert;
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
    upload_roots: Vec<PathBuf>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<PendingOriginal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle: Option<Vec<ArtifactFile>>,
}

/// Upload bytes belong to the decision, never to the caller's temporary path.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingOriginal {
    relative: String,
    bytes: Vec<u8>,
}

/// A journal as read from disk, before its version gate is applied.
/// Version-1 journals predate `kind` and are always documents; version-2
/// journals must carry `kind` explicitly. Defaulting a missing `kind` on
/// a version-2 journal would silently change what the journal means, so
/// recovery refuses it instead. Version 3 adds a document's original upload;
/// earlier readers refuse that version rather than dropping its bytes.
/// Version 4 binds an artifact document and every immutable companion byte.
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
    #[serde(default)]
    original: Option<PendingOriginal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle: Option<Vec<ArtifactFile>>,
}

impl RawPendingWrite {
    /// Apply the version gate and produce the replayable decision.
    fn into_pending(self, path: &Path) -> Result<PendingWrite> {
        let kind = match (self.version, self.kind, &self.original, &self.bundle) {
            (1, None | Some(WriteKind::Document), None, None) => WriteKind::Document,
            (2, Some(WriteKind::Document), None, None) => WriteKind::Document,
            (2, Some(WriteKind::Settings), None, None) => WriteKind::Settings,
            (3, Some(WriteKind::Document), Some(_), None) => WriteKind::Document,
            (4, Some(WriteKind::Artifact), None, Some(_)) => WriteKind::Artifact,
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
            original: self.original,
            bundle: self.bundle,
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WriteKind {
    #[default]
    Document,
    Settings,
    Artifact,
}

impl LibraryStore {
    /// Refuse `library_root` unless it is inside `{{workspace}}/library` on
    /// the physical path. Creates nothing.
    ///
    /// `initialize_at` and `RealLibrary` share this decision (STD-02 R24,
    /// STD-05 R6). `open` does not know the workspace, so the product
    /// adapter calls this before it. Roots a test created for itself still
    /// use `open` directly.
    pub fn confine_to_grant(library_root: &Path, workspace_root: &Path) -> Result<()> {
        resolve_within_grant(library_root, workspace_root)?;
        Ok(())
    }

    /// Open the Library and `<state_dir>/index.sqlite`, creating the layout
    /// and index as needed. Does not initialize git or create settings.
    /// An interrupted decided write is recovered before this returns.
    ///
    /// Does not apply the workspace grant. Product calls go through
    /// [`Self::confine_to_grant`] first; a root outside
    /// `{{workspace}}/library` must not reach this function.
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
        let upload_roots = root
            .parent()
            .map(|parent| vec![parent.to_path_buf()])
            .unwrap_or_default();
        Ok(Self {
            files: LibraryFs::new(root),
            index,
            state,
            git,
            upload_roots,
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
        // The requested string, when set, replaces the context root. The
        // grant decision itself is shared with every later open.
        let candidate = match requested {
            Some(dir) if !dir.trim().is_empty() => {
                let path = Path::new(dir);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    workspace_root.join(path)
                }
            }
            _ => library_root.to_path_buf(),
        };
        let granted = resolve_within_grant(&candidate, workspace_root)?;
        let candidate = granted.candidate;
        let allowed = granted.allowed;
        // Refuse a missing git before creating anything.
        let probe_cwd = if workspace_root.exists() {
            workspace_root.to_path_buf()
        } else {
            PathBuf::from("/")
        };
        Git::ensure_available(&probe_cwd)?;

        // Snapshot the layout the report lists before the writer state is
        // bootstrapped: the default state dir lives under `.office`, so
        // creating it first would hide `.office` from `created`.
        let missing_layout: Vec<String> = LAYOUT_DIRS
            .iter()
            .filter(|dir| !candidate.join(dir).exists())
            .map(|dir| (*dir).to_string())
            .collect();

        // Bootstrap only the lock directory; Library and index writes wait
        // until this guard is held, and completeness is checked under it.
        std::fs::create_dir_all(state_dir)
            .map_err(|e| Error::io("create writer state", state_dir, e))?;
        let lock = WriterLock::acquire(state_dir)?;
        // Create and verify the same resolved destination before using it.
        // Do not follow a final symlink, including one introduced since the
        // initial resolution.
        if std::fs::symlink_metadata(&candidate)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(Error::UnsafePath {
                kind: "symlink",
                value: candidate.display().to_string(),
            });
        }
        LibraryFs::create_private_dirs(&candidate)?;
        let resolved = candidate
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", &candidate, e))?;
        if resolved != candidate {
            return Err(Error::LibraryDirOutside {
                requested: resolved,
                allowed,
            });
        }
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

        let mut created = missing_layout;
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
        let pending = self.decide(
            WriteKind::Settings,
            ".office/config.yaml",
            &text,
            None,
            None,
        )?;
        self.finish(&pending)
    }

    /// Record a private-endpoint check in the latest config while holding
    /// the document writer lock (`Settings::with_route_check`).
    pub fn record_route_check(
        &self,
        endpoint: &str,
        model: &str,
        capabilities: &[office_core::domain::endpoint::Capability],
        verified_at: &str,
    ) -> Result<()> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let settings =
            self.settings()?
                .with_route_check(endpoint, model, capabilities, verified_at)?;
        let text = settings.to_yaml()?;
        self.files.validate_settings_path()?;
        let pending = self.decide(
            WriteKind::Settings,
            ".office/config.yaml",
            &text,
            None,
            None,
        )?;
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
        // Scan before rebuild. `rebuild` deletes every indexed row in one
        // transaction, so an incomplete walk must return here and leave the
        // previous index in place.
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
        let pending = self.decide(WriteKind::Document, relative, text, None, None)?;
        self.finish(&pending)?;
        self.files.read_document(Path::new(relative))
    }

    /// Promote an entire artifact while holding the writer lock across version
    /// allocation, durable decision, byte publication, index and one commit.
    pub fn promote_artifact(
        &self,
        candidate: &Artifact,
        files: &[ArtifactFile],
    ) -> Result<Artifact> {
        candidate
            .validate_bundle(files)
            .map_err(office_core::Error::from)?;
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let history = self
            .files
            .read_artifact_markdown()?
            .into_iter()
            .map(|(path, text)| {
                Artifact::from_document(&path, &text)
                    .map_err(office_core::Error::from)
                    .map_err(Error::from)
            })
            .collect::<Result<Vec<_>>>()?;
        let path = candidate.path().map_err(office_core::Error::from)?;
        let month = Path::new(&path)
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| Error::Journal {
                path: self.journal_path(),
            })?;
        let occupied = self.files.entry_names(month)?;
        let artifact = candidate
            .next_version(&history, |id| {
                occupied.iter().any(|name| name.eq_ignore_ascii_case(id))
            })
            .map_err(office_core::Error::from)?;
        let relative = artifact.path().map_err(office_core::Error::from)?;
        let text = artifact.to_markdown().map_err(office_core::Error::from)?;
        self.validate_identity(&DocumentRecord::from_markdown(Path::new(&relative), &text))?;
        let pending = self.decide(
            WriteKind::Artifact,
            &relative,
            &text,
            None,
            Some(files.to_vec()),
        )?;
        self.finish(&pending)?;
        Ok(artifact)
    }

    /// Read every artifact frontmatter file, without truncating series history.
    pub fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        Ok(self
            .files
            .read_artifact_markdown()?
            .into_iter()
            .map(|(path, markdown)| LibraryText {
                path,
                markdown: Some(markdown),
            })
            .collect())
    }

    fn validate_bundle(&self, pending: &PendingWrite) -> Result<()> {
        let Some(files) = &pending.bundle else {
            return Ok(());
        };
        if !matches!(pending.kind, WriteKind::Artifact)
            || pending.version != 4
            || pending.original.is_some()
        {
            return Err(Error::Journal {
                path: self.journal_path(),
            });
        }
        let artifact = Artifact::from_document(&pending.relative, &pending.text)
            .map_err(office_core::Error::from)?;
        artifact
            .validate_bundle(files)
            .map_err(office_core::Error::from)?;
        let relative = Path::new(&pending.relative);
        self.files.validate_document_path(relative)?;
        let parent = relative.parent().ok_or_else(|| Error::Journal {
            path: self.journal_path(),
        })?;
        for file in files {
            self.files
                .validate_relative_path(&parent.join(&file.path))?;
        }
        Ok(())
    }

    fn decide(
        &self,
        kind: WriteKind,
        relative: &str,
        text: &str,
        original: Option<PendingOriginal>,
        bundle: Option<Vec<ArtifactFile>>,
    ) -> Result<PendingWrite> {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| {
            Error::io(
                "read write decision time",
                self.library_root(),
                std::io::Error::other(e),
            )
        })?;
        let pending = PendingWrite {
            version: if bundle.is_some() {
                4
            } else if original.is_some() {
                3
            } else {
                2
            },
            kind,
            root: self.library_root().into(),
            relative: relative.into(),
            text: text.into(),
            token: format!("{}-{}", std::process::id(), time.as_nanos()),
            original,
            bundle,
        };
        self.validate_original(&pending)?;
        self.validate_bundle(&pending)?;
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

    /// A version-3 upload can publish only the named original beside its
    /// source document, with the digest and byte count the document records.
    fn validate_original(&self, pending: &PendingWrite) -> Result<()> {
        let Some(original) = &pending.original else {
            return Ok(());
        };
        let source = Path::new(&pending.relative);
        let relative = Path::new(&original.relative);
        self.files.validate_relative_path(relative)?;
        let meta = parse_source_meta(&pending.text);
        if !source.starts_with("sources")
            || source.file_name().is_none_or(|name| name != "source.md")
            || source.parent() != relative.parent()
            || !relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("original.") && meta.original_file.as_deref() == Some(name)
                })
            || meta.sha256.as_deref() != Some(sha256_hex(&original.bytes).as_str())
            || u64::try_from(original.bytes.len())
                .ok()
                .is_none_or(|count| meta.bytes != Some(count))
        {
            return Err(Error::Journal {
                path: self.journal_path(),
            });
        }
        Ok(())
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
        if !(1..=4).contains(&pending.version)
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
            WriteKind::Artifact => {
                self.require_initialized()?;
                self.validate_bundle(&pending)?;
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
        self.validate_original(&pending)?;
        self.finish(&pending)
    }

    fn finish(&self, pending: &PendingWrite) -> Result<()> {
        match pending.kind {
            WriteKind::Document => {
                let relative = Path::new(&pending.relative);
                if let Some(original) = &pending.original {
                    self.files
                        .write_original(Path::new(&original.relative), &original.bytes)?;
                }
                self.files.write_document(relative, &pending.text)?;
                let record = self.files.read_document(relative)?;
                self.index.upsert(&record)?;
            }
            WriteKind::Artifact => {
                self.validate_bundle(pending)?;
                let files = pending.bundle.as_deref().ok_or_else(|| Error::Journal {
                    path: self.journal_path(),
                })?;
                self.files.publish_artifact_bundle(
                    Path::new(&pending.relative),
                    &pending.text,
                    files,
                    &pending.token,
                )?;
                self.index
                    .upsert(&self.files.read_document(Path::new(&pending.relative))?)?;
            }
            WriteKind::Settings => self.files.write_settings(&pending.text)?,
        }
        let mut paths = vec![pending.relative.clone()];
        if let Some(files) = &pending.bundle {
            let parent = Path::new(&pending.relative)
                .parent()
                .ok_or_else(|| Error::Journal {
                    path: self.journal_path(),
                })?;
            paths.extend(
                files
                    .iter()
                    .map(|file| parent.join(&file.path).to_string_lossy().to_string()),
            );
        }
        self.git.commit_paths(&paths, &pending.token)?;
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

    /// Indexed documents that cite or relate to `id`. No writer-lock wait.
    pub fn backlinks(&self, id: &str) -> Result<Vec<Backlink>> {
        self.read_ready()?;
        self.index.backlinks(id)
    }

    /// Every knowledge page file, in path order, read from disk (the
    /// source of truth) without waiting for the writer lock.
    pub fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        Ok(self
            .files
            .read_knowledge_markdown()?
            .into_iter()
            .map(|(path, markdown)| LibraryText {
                path,
                markdown: Some(markdown),
            })
            .collect())
    }

    /// Every template file, in path order. A symlinked or unreadable file
    /// is listed with no Markdown so a caller can name it.
    pub fn templates(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        Ok(self
            .files
            .read_templates()?
            .into_iter()
            .map(|template| LibraryText {
                path: template.relative,
                markdown: template.markdown,
            })
            .collect())
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

    /// Set the canonical roots upload/transcript paths must stay inside.
    #[must_use]
    pub fn with_upload_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.upload_roots = roots;
        self
    }

    fn read_contained(&self, raw: &str) -> std::result::Result<Vec<u8>, office_core::Error> {
        let canonical = std::fs::canonicalize(raw).map_err(|_| office_core::Error::Library {
            code: "upload_not_found",
            message: format!("upload path {raw} could not be read"),
        })?;
        let inside = self
            .upload_roots
            .iter()
            .any(|root| std::fs::canonicalize(root).is_ok_and(|root| canonical.starts_with(root)));
        if !inside {
            return Err(office_core::Error::Library {
                code: "upload_outside_allowed_roots",
                message: format!("upload path {raw} is outside the allowed roots"),
            });
        }
        if Path::new(raw)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| convert::is_document(&ext.to_ascii_lowercase()))
        {
            use std::io::Read;
            let file = std::fs::File::open(&canonical)
                .map_err(|e| office_core::Error::from(Error::io("read document", &canonical, e)))?;
            let mut bytes = Vec::new();
            file.take(convert::INPUT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| office_core::Error::from(Error::io("read document", &canonical, e)))?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > convert::INPUT_LIMIT {
                return Err(Error::ConversionLimit {
                    limit: "32 MiB document input",
                }
                .into());
            }
            return Ok(bytes);
        }
        std::fs::read(&canonical).map_err(|_| office_core::Error::Library {
            code: "upload_not_found",
            message: format!("upload path {raw} could not be read"),
        })
    }

    fn source_views(&self) -> Result<Vec<(String, String)>> {
        // (path, raw markdown) for every indexed source, in path order.
        let docs = self.files.scan_documents()?;
        let mut out = Vec::new();
        for doc in docs {
            if doc.path.starts_with("sources/") {
                let text = self.files.read_text(Path::new(&doc.path))?;
                out.push((doc.path, text));
            }
        }
        Ok(out)
    }

    /// Capture a source: copy an upload, store pasted text, open a
    /// conversation, or append to one (see domain rules in office-core).
    pub fn capture_source(
        &self,
        request: &CaptureSourceRequest,
    ) -> office_core::Result<SourceOutcome> {
        office_core::domain::source::validate_capture_tags(&request.tags)?;
        // Contained reads happen before any write, so a refused path
        // writes nothing.
        let mut upload_bytes: Option<Vec<u8>> = None;
        let mut upload_ext = String::new();
        let mut upload_stem = String::new();
        if let Some(raw) = &request.upload_path {
            let bytes = self.read_contained(raw)?;
            let path = Path::new(raw);
            upload_ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            upload_stem = path
                .file_stem()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            upload_bytes = Some(bytes);
        }
        let mut transcript_body: Option<String> = None;
        if let Some(raw) = &request.transcript_path {
            if !raw.to_ascii_lowercase().ends_with(".vtt") {
                return Err(office_core::Error::InvalidArgument(
                    "transcript_path must name a .vtt file".to_string(),
                ));
            }
            let bytes = self.read_contained(raw)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                office_core::Error::InvalidArgument(
                    "transcript_path must be UTF-8 WebVTT".to_string(),
                )
            })?;
            transcript_body = Some(vtt_to_body(&text));
        }
        let converted = if transcript_body.is_none() {
            upload_bytes
                .as_ref()
                .map(|bytes| convert::document(&upload_ext, bytes))
                .transpose()
                .map_err(office_core::Error::from)?
                .flatten()
        } else {
            None
        };
        let _lock = WriterLock::acquire(&self.state).map_err(office_core::Error::from)?;
        self.recover().map_err(office_core::Error::from)?;
        self.require_initialized()
            .map_err(office_core::Error::from)?;
        let settings = self.settings().map_err(office_core::Error::from)?;
        let existing = self.source_views().map_err(office_core::Error::from)?;
        if let Some(target) = &request.append_to {
            if request.entries.is_empty() {
                return Err(office_core::Error::InvalidArgument(
                    "capture append_to requires at least one entry".to_string(),
                ));
            }
            let mut lines = Vec::new();
            for entry in &request.entries {
                lines.push(format_conversation_entry(
                    &request.captured_at,
                    &entry.kind,
                    &entry.text,
                )?);
            }
            let Some((path, raw_text)) = existing
                .iter()
                .find(|(_, text)| parse_source_meta(text).id == *target)
            else {
                return Err(office_core::Error::Library {
                    code: "source_not_found",
                    message: format!("source {target} was not found"),
                });
            };
            let meta = parse_source_meta(raw_text);
            if meta.kind != "conversation" {
                return Err(office_core::Error::Library {
                    code: "not_a_conversation_source",
                    message: format!("source {target} is not a conversation source"),
                });
            }
            let mut body = source_body(raw_text);
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(&lines.join("\n"));
            body.push('\n');
            // Rebuild the document with identical frontmatter bytes: take
            // everything up to and including the closing fence verbatim.
            let head_end = raw_text.find("\n---").map(|i| i + 4).unwrap_or(0);
            let mut new_text = String::new();
            new_text.push_str(&raw_text[..head_end]);
            if !raw_text[..head_end].ends_with('\n') {
                new_text.push('\n');
            }
            new_text.push_str(&body);
            let new_text =
                office_core::domain::source::merge_source_tags(&new_text, &request.tags)?
                    .unwrap_or(new_text);
            let pending = self
                .decide(WriteKind::Document, path, &new_text, None, None)
                .map_err(office_core::Error::from)?;
            self.finish(&pending).map_err(office_core::Error::from)?;
            return Ok(SourceOutcome {
                id: meta.id,
                kind: meta.kind,
                title: meta.title,
                path: path.clone(),
                sha256: meta.sha256,
                bytes: meta.bytes,
                media_type: meta.media_type,
                sensitivity: if meta.sensitivity.is_empty() {
                    "normal".into()
                } else {
                    meta.sensitivity
                },
                deduplicated: false,
                appended: lines.len(),
                version: meta.version,
            });
        }
        let explicit_kind = match &request.kind {
            Some(k) => Some(
                SourceKind::parse(k).ok_or_else(|| office_core::Error::Library {
                    code: "invalid_source_kind",
                    message: format!("unknown source kind {k:?}"),
                })?,
            ),
            None => None,
        };
        let kind = explicit_kind.unwrap_or_else(|| {
            if is_video_extension(&upload_ext) {
                SourceKind::Video
            } else if convert::is_document(&upload_ext) {
                SourceKind::Doc
            } else if !request.entries.is_empty() {
                SourceKind::Conversation
            } else {
                SourceKind::Note
            }
        });
        let (body, sha, bytes_len, media_type, derived) = if let Some(body) = transcript_body {
            (
                body,
                upload_bytes.as_ref().map(|b| sha256_hex(b)),
                upload_bytes.as_ref().map(|b| b.len() as u64),
                upload_bytes.as_ref().map(|_| {
                    media_type_for_extension(&upload_ext)
                        .unwrap_or("application/octet-stream")
                        .to_string()
                }),
                Some("vtt-import"),
            )
        } else if let Some(bytes) = &upload_bytes {
            let media = media_type_for_extension(&upload_ext)
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = converted.unwrap_or_default();
            (
                body,
                Some(sha256_hex(bytes)),
                Some(bytes.len() as u64),
                Some(media),
                None,
            )
        } else if kind == SourceKind::Conversation && !request.entries.is_empty() {
            let mut lines = Vec::new();
            for entry in &request.entries {
                lines.push(format_conversation_entry(
                    &request.captured_at,
                    &entry.kind,
                    &entry.text,
                )?);
            }
            let body = lines.join("\n");
            (
                body.clone(),
                Some(sha256_hex(normalize_text(&body).as_bytes())),
                None,
                None,
                None,
            )
        } else {
            let text = request.text.clone().unwrap_or_default();
            if text.trim().is_empty() {
                return Err(office_core::Error::InvalidArgument(
                    "capture requires text, upload_path, entries or append_to".to_string(),
                ));
            }
            let normalized = normalize_text(&text);
            (
                normalized.clone(),
                Some(sha256_hex(normalized.as_bytes())),
                None,
                None,
                None,
            )
        };
        if let Some(sha_value) = &sha {
            for (path, text) in &existing {
                let meta = parse_source_meta(text);
                let text_sha = sha256_hex(normalize_text(&source_body(text)).as_bytes());
                if meta.sha256.as_ref() == Some(sha_value) || &text_sha == sha_value {
                    if let Some(markdown) =
                        office_core::domain::source::merge_source_tags(text, &request.tags)?
                    {
                        let pending = self
                            .decide(WriteKind::Document, path, &markdown, None)
                            .map_err(office_core::Error::from)?;
                        self.finish(&pending).map_err(office_core::Error::from)?;
                    }
                    return Ok(SourceOutcome {
                        id: meta.id,
                        kind: meta.kind,
                        title: meta.title,
                        path: path.clone(),
                        sha256: meta.sha256.or_else(|| Some(sha_value.clone())),
                        bytes: meta.bytes,
                        media_type: meta.media_type,
                        sensitivity: if meta.sensitivity.is_empty() {
                            "normal".into()
                        } else {
                            meta.sensitivity
                        },
                        deduplicated: true,
                        appended: 0,
                        version: meta.version,
                    });
                }
            }
        }
        let title = request
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| {
                if !upload_stem.is_empty() {
                    upload_stem.replace(['-', '_'], " ")
                } else {
                    body.lines()
                        .next()
                        .unwrap_or(kind.as_str())
                        .chars()
                        .take(60)
                        .collect()
                }
            });
        let base = source_id_base(&request.captured_at, &title);
        let mut known_ids: Vec<String> = existing
            .iter()
            .map(|(_, text)| parse_source_meta(text).id)
            .collect();
        let date = &request.captured_at;
        let (yyyy, mm) = if date.len() >= 7 {
            (&date[0..4], &date[5..7])
        } else {
            ("1970", "01")
        };
        let month = format!("sources/{yyyy}/{mm}");
        known_ids.extend(
            self.files
                .entry_names(Path::new(&month))
                .map_err(office_core::Error::from)?,
        );
        let id = unique_source_id(&base, |candidate| known_ids.iter().any(|k| k == candidate));
        let rel_dir = format!("{month}/{id}");
        let rel_path = format!("{rel_dir}/source.md");
        let sensitivity = match &request.sensitivity {
            Some(s) => Sensitivity::parse(s).ok_or_else(|| {
                office_core::Error::InvalidArgument(
                    "sensitivity must be normal or private".to_string(),
                )
            })?,
            None => settings
                .sensitivity
                .get(kind.as_str())
                .copied()
                .unwrap_or_default(),
        };
        let original_name = upload_bytes
            .as_ref()
            .map(|_| format!("original.{upload_ext}"));
        let markdown = render_source(&RenderSource {
            id: &id,
            kind,
            title: &title,
            captured_at: &request.captured_at,
            captured_by: request
                .captured_by
                .as_deref()
                .unwrap_or(if upload_bytes.is_some() {
                    "upload"
                } else {
                    "paste"
                }),
            origin: request.origin.as_deref(),
            occurred_at: request.occurred_at.as_deref(),
            participants: &request.participants,
            tags: &request.tags,
            sensitivity,
            original: match (&original_name, &sha, bytes_len, &media_type) {
                (Some(name), Some(s), Some(b), Some(m)) => {
                    Some((name.as_str(), s.as_str(), b, m.as_str()))
                }
                _ => None,
            },
            derived_transcription: derived,
            retention_days: settings.retention.get(kind.as_str()).copied().flatten(),
            body: &body,
        });
        self.files
            .validate_document_path(Path::new(&rel_path))
            .map_err(office_core::Error::from)?;
        self.validate_identity(&DocumentRecord::from_markdown(
            Path::new(&rel_path),
            &markdown,
        ))
        .map_err(office_core::Error::from)?;
        let original = upload_bytes
            .zip(original_name)
            .map(|(bytes, name)| PendingOriginal {
                relative: format!("{rel_dir}/{name}"),
                bytes,
            });
        let pending = self
            .decide(WriteKind::Document, &rel_path, &markdown, original, None)
            .map_err(office_core::Error::from)?;
        self.finish(&pending).map_err(office_core::Error::from)?;
        Ok(SourceOutcome {
            id,
            kind: kind.as_str().to_string(),
            title,
            path: rel_path,
            sha256: sha,
            bytes: bytes_len,
            media_type,
            sensitivity: sensitivity.as_str().to_string(),
            deduplicated: false,
            appended: 0,
            version: 1,
        })
    }

    /// Read one source by id.
    pub fn read_source(&self, id: &str) -> office_core::Result<SourceView> {
        self.read_ready().map_err(office_core::Error::from)?;
        let existing = self.source_views().map_err(office_core::Error::from)?;
        let Some((path, text)) = existing
            .iter()
            .find(|(_, text)| parse_source_meta(text).id == id)
        else {
            return Err(office_core::Error::Library {
                code: "source_not_found",
                message: format!("source {id} was not found"),
            });
        };
        let meta = parse_source_meta(text);
        Ok(SourceView {
            id: meta.id,
            kind: meta.kind,
            title: meta.title,
            path: path.clone(),
            body: source_body(text).trim_end_matches('\n').to_string(),
            sensitivity: if meta.sensitivity.is_empty() {
                "normal".into()
            } else {
                meta.sensitivity
            },
            sha256: meta.sha256,
            bytes: meta.bytes,
            media_type: meta.media_type,
            version: meta.version,
        })
    }

    /// A document's current Markdown, frontmatter included. No writer-lock wait.
    pub fn read_text(&self, relative: &Path) -> Result<String> {
        self.read_ready()?;
        self.files.validate_document_path(relative)?;
        self.files.read_text(relative)
    }

    /// The Library repository's current commit, `None` before the first.
    pub fn head_commit(&self) -> Result<Option<String>> {
        self.read_ready()?;
        self.git.head()
    }

    /// Write one attachment copy under `.office/attach/<call>/<name>`, outside
    /// the indexed layout, and return its absolute path. `name` is the
    /// Library-relative path the copy stands for. The staging tree lives in
    /// the Library because the plugin's only workspace write grant is
    /// `{{workspace}}/library` and `orbit.task.artifact.put` reads only files
    /// inside the workspace. Writes go through `--only` commits, so staged
    /// files never enter a Library commit.
    pub fn stage_attachment(&self, name: &str, text: &str) -> Result<PathBuf> {
        self.read_ready()?;
        self.files.validate_relative_path(Path::new(name))?;
        let staging = self.staging_root()?;
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let path = staging
            .join(format!("{}-{}", std::process::id(), time.as_nanos()))
            .join(name);
        if let Some(parent) = path.parent() {
            LibraryFs::create_private_dirs(parent)?;
        }
        write_atomic(&path, text.as_bytes())?;
        Ok(path)
    }

    /// `.office/attach`, created if needed; a symlink on the way is refused.
    fn staging_root(&self) -> Result<PathBuf> {
        let mut physical = self.library_root().to_path_buf();
        for part in Path::new(STAGING_DIR).components() {
            physical.push(part);
            match std::fs::symlink_metadata(&physical) {
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(Error::UnsafePath {
                        kind: "staging directory",
                        value: STAGING_DIR.to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    LibraryFs::create_private_dirs(&physical)?;
                }
                Err(e) => return Err(Error::io("inspect staging directory", &physical, e)),
            }
        }
        Ok(physical)
    }

    /// Remove a file [`Self::stage_attachment`] wrote, and its emptied
    /// staging directories. Any other path is refused.
    pub fn remove_staged(&self, staged: &Path) -> Result<()> {
        let staging = self.library_root().join(STAGING_DIR);
        let inside = staged.starts_with(&staging)
            && staged != staging
            && staged.components().all(|part| {
                matches!(
                    part,
                    std::path::Component::Normal(_)
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            });
        if !inside {
            return Err(Error::UnsafePath {
                kind: "staged attachment",
                value: staged.display().to_string(),
            });
        }
        match std::fs::symlink_metadata(staged) {
            Ok(metadata) if metadata.is_file() => std::fs::remove_file(staged)
                .map_err(|e| Error::io("remove staged attachment", staged, e))?,
            Ok(_) => {
                return Err(Error::UnsafePath {
                    kind: "staged attachment",
                    value: staged.display().to_string(),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io("inspect staged attachment", staged, e)),
        }
        let mut dir = staged.parent();
        while let Some(current) = dir {
            if current == staging || !current.starts_with(&staging) || !dir_is_empty(current) {
                break;
            }
            if std::fs::remove_dir(current).is_err() {
                break;
            }
            dir = current.parent();
        }
        Ok(())
    }

    /// SHA-256 of a Library file, including an `original.*` upload.
    pub fn file_sha256(&self, relative: &Path) -> Result<String> {
        self.read_ready()?;
        self.files.validate_relative_path(relative)?;
        Ok(sha256_file(&self.library_root().join(relative))?)
    }
}

/// Library-relative staging root for `office.attach` copies (never indexed).
const STAGING_DIR: &str = ".office/attach";

/// A Library root that resolved inside `{{workspace}}/library`.
struct GrantedRoot {
    candidate: PathBuf,
    allowed: PathBuf,
}

/// Decide containment on the physical path before any create (STD-05 R6).
///
/// Lexical `..` and absolute paths are normalized first so a join cannot
/// spell a different root. A missing suffix is appended to its canonical
/// ancestor; the spelled path alone is never the decision.
fn resolve_within_grant(candidate: &Path, workspace_root: &Path) -> Result<GrantedRoot> {
    let allowed = normalize_path(&workspace_root.join("library"));
    let candidate = normalize_path(candidate);
    if candidate != allowed && !candidate.starts_with(&allowed) {
        return Err(Error::LibraryDirOutside {
            requested: candidate,
            allowed,
        });
    }
    // Resolve the grant and destination before creating the lock, index or
    // Library. A missing suffix is appended to its canonical ancestor.
    let allowed = resolve_existing_ancestor(&allowed)?;
    if std::fs::symlink_metadata(&candidate).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(Error::UnsafePath {
            kind: "symlink",
            value: candidate.display().to_string(),
        });
    }
    let candidate = resolve_existing_ancestor(&candidate)?;
    if !candidate.starts_with(&allowed) {
        return Err(Error::LibraryDirOutside {
            requested: candidate,
            allowed,
        });
    }
    Ok(GrantedRoot { candidate, allowed })
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

/// Resolve an existing ancestor without treating dangling links as missing.
fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = ancestor
                    .canonicalize()
                    .map_err(|e| Error::io("resolve Library path", ancestor, e))?;
                for name in missing.into_iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| Error::io("resolve Library path", ancestor, e))?;
                missing.push(name);
                ancestor = ancestor.parent().ok_or_else(|| {
                    Error::io(
                        "resolve Library path",
                        ancestor,
                        std::io::Error::other("no existing ancestor"),
                    )
                })?;
            }
            Err(e) => return Err(Error::io("inspect Library path", ancestor, e)),
        }
    }
}

fn dir_is_empty(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

impl office_core::ports::Library for LibraryStore {
    fn promote_artifact(
        &self,
        artifact: &Artifact,
        files: &[ArtifactFile],
    ) -> office_core::Result<Artifact> {
        Ok(Self::promote_artifact(self, artifact, files)?)
    }
    fn artifact_documents(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(Self::artifact_documents(self)?)
    }

    fn read_document(&self, relative: &Path) -> office_core::Result<DocumentRecord> {
        Ok(Self::read_document(self, relative)?)
    }
    fn write_document(&self, relative: &Path, text: &str) -> office_core::Result<DocumentRecord> {
        Ok(Self::write_document(self, relative, text)?)
    }
    fn get(&self, id: &str) -> office_core::Result<Option<DocumentRecord>> {
        Ok(Self::get(self, id)?)
    }
    fn backlinks(&self, id: &str) -> office_core::Result<Vec<Backlink>> {
        Ok(Self::backlinks(self, id)?)
    }
    fn knowledge_pages(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(Self::knowledge_pages(self)?)
    }
    fn templates(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(Self::templates(self)?)
    }
    fn search(&self, query: &str, filter: &SearchFilter) -> office_core::Result<Vec<SearchHit>> {
        Ok(Self::search(self, query, filter)?)
    }
    fn history(&self, relative: &Path) -> office_core::Result<Vec<Revision>> {
        Ok(Self::history(self, relative)?)
    }
    fn read_text(&self, relative: &Path) -> office_core::Result<String> {
        Ok(Self::read_text(self, relative)?)
    }
    fn head_commit(&self) -> office_core::Result<Option<String>> {
        Ok(Self::head_commit(self)?)
    }
    fn stage_attachment(&self, name: &str, text: &str) -> office_core::Result<String> {
        let staged = Self::stage_attachment(self, name, text)?;
        staged
            .to_str()
            .map(str::to_string)
            .ok_or(office_core::Error::Library {
                code: "unsafe_path",
                message: "staged attachment path is not UTF-8".to_string(),
            })
    }
    fn remove_staged(&self, staged: &str) -> office_core::Result<()> {
        Ok(Self::remove_staged(self, Path::new(staged))?)
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
    fn record_route_check(
        &self,
        endpoint: &str,
        model: &str,
        capabilities: &[office_core::domain::endpoint::Capability],
        verified_at: &str,
    ) -> office_core::Result<()> {
        Ok(Self::record_route_check(
            self,
            endpoint,
            model,
            capabilities,
            verified_at,
        )?)
    }
    fn capture_source(&self, request: &CaptureSourceRequest) -> office_core::Result<SourceOutcome> {
        Self::capture_source(self, request)
    }
    fn read_source(&self, id: &str) -> office_core::Result<SourceView> {
        Self::read_source(self, id)
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
