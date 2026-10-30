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

use office_core::domain::knowledge::{DocumentKind, LibraryLayer};
use office_core::domain::template::{KnowledgePage, TemplateFile, assess_templates};
use office_core::ports::{InitOutcome, LibraryHealth, LibraryText};

use crate::contracts::{Backlink, DocumentRecord, Revision, SearchFilter, SearchHit};
use crate::driver::convert;
use crate::driver::file::library::LAYOUT_DIRS;
use crate::driver::file::{library::LibraryFs, lock::WriterLock};
use crate::driver::git::Git;
use crate::driver::sqlite::connection::IndexStore;
use crate::{Error, Result};

mod grant;
mod pending;

use grant::{normalize_path, resolve_existing_ancestor, resolve_within_grant};
use pending::{
    PendingOriginal, PendingWrite, RawPendingWrite, WriteKind, normalize_upload_extension,
};

const LATEST_PENDING_WRITE_VERSION: u32 = 8;

/// A Library and its index. Construction may create layout/index directories;
/// read methods never create state or wait for the writer lock. Bounded media
/// reads hold a nonblocking shared probe to serialize privacy changes.
pub struct LibraryStore {
    files: LibraryFs,
    index: IndexStore,
    state: PathBuf,
    git: Git,
    upload_roots: Vec<PathBuf>,
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

    /// Open only for bootstrap file reads. No layout, disk index, writer lock,
    /// migration, backfill or journal recovery is performed. An abandoned
    /// write refuses through read_ready; a missing index/state stays missing.
    pub fn open_for_bootstrap_read(library_root: &Path, state_dir: &Path) -> Result<Self> {
        let root = library_root
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", library_root, e))?;
        let store = Self {
            files: LibraryFs::new(root.clone()),
            index: IndexStore::open_in_memory()?,
            state: state_dir.to_path_buf(),
            git: Git::new(&root, state_dir),
            upload_roots: Vec::new(),
        };
        store.read_ready()?;
        Ok(store)
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
        // A schema upgrade that added template/date left pre-upgrade rows
        // NULL in those columns; template and date come from file
        // frontmatter, so only a rescan can fill them. Do it here, under
        // the writer lock, before any search can silently miss those
        // rows. The durable marker (migration.rs) survives a failed
        // rescan, so the next open retries instead of never healing.
        if store.index.needs_backfill()? {
            let lock = WriterLock::acquire(&store.state)?;
            store.reindex_locked(&lock)?;
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
    /// missing and preserves existing config, templates and pages. Appends
    /// a missing `frames/` or `.office/state/` to an existing `.gitignore`
    /// when needed, preserving other lines.
    /// A complete Library, including every bundled template file, reports
    /// `already_initialized` with no commit.
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

        // The default writer state lives beneath .office. Claim that one
        // directory atomically at bootstrap, before the lock exists; all
        // other layout attribution is decided while holding the lock.
        let office = candidate.join(".office");
        let absolute_state = std::path::absolute(state_dir)
            .map_err(|e| Error::io("resolve writer state", state_dir, e))?;
        let resolved_state = resolve_existing_ancestor(&normalize_path(&absolute_state))?;
        let mut bootstrapped_office = false;
        if resolved_state.starts_with(&office) {
            LibraryFs::create_private_dirs(&candidate)?;
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&office) {
                Ok(()) => bootstrapped_office = true,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(Error::io("create Library directory", &office, e)),
            }
        }
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
        let gitignore = files.gitignore_text()?;
        let gitignore_exists = gitignore.is_some();
        let gitignore_text = gitignore.unwrap_or_default();
        let frames_ignored = gitignore_text.lines().any(|line| line == "frames/");
        let state_ignored = gitignore_text
            .lines()
            .any(|line| line == ".office/state/" || line == ".office/state");
        let state_tracked = if is_repo {
            git.has_tracked(".office/state")?
        } else {
            false
        };
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
        // Move legacy sources into the kind-partitioned layout before the
        // completeness check, so a complete-but-legacy Library migrates
        // instead of reporting `already_initialized`. The writer lock is
        // already held; reindex and the commit below finish the move.
        let migrated_sources = if candidate.exists() {
            files.migrate_legacy_sources()?
        } else {
            Vec::new()
        };
        if is_repo
            && config_text.is_some()
            && gitignore_exists
            && frames_ignored
            && state_ignored
            && !state_tracked
            && index_exists
            && layout_complete
            && templates_present
            && migrated_sources.is_empty()
        {
            return Ok(InitOutcome {
                already_initialized: true,
                created: Vec::new(),
            });
        }

        let mut created: Vec<String> = LAYOUT_DIRS
            .iter()
            .filter(|dir| {
                !candidate.join(dir).exists() || (**dir == ".office" && bootstrapped_office)
            })
            .map(|dir| (*dir).to_string())
            .collect();
        files.ensure_layout()?;
        for (_, new_dir) in &migrated_sources {
            if !created.contains(new_dir) {
                created.push(new_dir.clone());
            }
        }
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
        if !gitignore_exists || !frames_ignored || !state_ignored {
            let mut text = if gitignore_exists {
                gitignore_text
            } else {
                "original.*\n".into()
            };
            if !frames_ignored {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("frames/\n");
            }
            if !state_ignored {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(".office/state/\n");
            }
            write_atomic(&candidate.join(".gitignore"), text.as_bytes())?;
            created.push(".gitignore".to_string());
        }
        // Only the paths this call created are committed, so a later restore
        // does not stage an edit the user made to another template.
        // A tracked `.office/state` is removed from the index with
        // `git rm -r --cached` and committed together with the `.gitignore`
        // change. `--cached` never deletes the files on disk, and the
        // removal is scoped to `.office/state` so config and templates
        // stay tracked.
        created.extend(files.install_missing_templates()?);
        let untrack_state = state_tracked;
        if untrack_state {
            created.push(".office/state".to_string());
        }
        let root = candidate
            .canonicalize()
            .map_err(|e| Error::io("resolve Library", &candidate, e))?;
        let index = IndexStore::open(&state_dir.join("index.sqlite"))?;
        let store = Self::from_parts(root, state_dir, index)?;
        if !index_exists {
            created.push("index.sqlite".to_string());
        }
        store.reindex_locked(&lock)?;
        // `.office` itself is never added: `add --force -- .office`
        // would force-add the ignored `.office/state` it contains when
        // writer state lives inside the Library. Its tracked files
        // (`.office/config.yaml`, `.office/templates/...`) are each in
        // `created` on their own and are added individually.
        let add_paths: Vec<String> = created
            .iter()
            .filter(|path| {
                path.as_str() != ".git"
                    && path.as_str() != "index.sqlite"
                    && path.as_str() != ".office"
                    && path.as_str() != ".office/state"
            })
            .cloned()
            .collect();
        let mut add_paths = add_paths;
        for (old_dir, _) in &migrated_sources {
            let tracked = is_repo && git.has_tracked(old_dir)?;
            if tracked && !add_paths.contains(old_dir) {
                add_paths.push(old_dir.clone());
            }
        }
        store
            .git
            .commit_scaffold(&add_paths, untrack_state, "Initialize the Library")?;
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

    /// Move legacy `sources/YYYY/MM/<id>/` directories into the
    /// kind-partitioned layout, reindex and commit the move. Idempotent:
    /// a Library with no legacy sources returns an empty list and makes
    /// no commit. Reads accept both layouts, so this never has to run
    /// before a source can be read. Returns the new directories.
    pub fn migrate_source_layout(&self) -> Result<Vec<String>> {
        let lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let moved = self.files.migrate_legacy_sources()?;
        if moved.is_empty() {
            return Ok(Vec::new());
        }
        self.reindex_locked(&lock)?;
        let mut paths: Vec<String> = Vec::new();
        for (old, new) in &moved {
            // An untracked legacy directory has no deletion to stage, and
            // `git add` refuses a pathspec that never matched.
            if self.git.has_tracked(old)? {
                paths.push(old.clone());
            }
            paths.push(new.clone());
        }
        self.git.commit_scaffold(
            &paths,
            false,
            "Move sources into the kind-partitioned layout",
        )?;
        Ok(moved.into_iter().map(|(_, new)| new).collect())
    }

    fn reindex_locked(&self, _lock: &WriterLock) -> Result<u32> {
        self.recover()?;
        // Scan before rebuild. `rebuild` deletes every indexed row in one
        // transaction, so an incomplete walk must return here and leave the
        // previous index in place.
        let mut documents = self.files.scan_documents()?;
        documents.extend(
            self.shared_documents()?
                .into_iter()
                .map(|(record, _)| record),
        );
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

    /// The clone is optional. Reject links in every component from plugin state.
    fn shared_files(&self) -> Result<Option<LibraryFs>> {
        let state = LibraryFs::new(self.state.clone());
        match state.validate_relative_path(Path::new("shared/repo")) {
            Ok(()) => {}
            Err(Error::UnsafePath { .. }) => return Ok(None),
            Err(error) => return Err(error),
        }
        let root = self.state.join("shared/repo");
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() => Ok(Some(LibraryFs::new(root))),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("inspect shared clone", root, error)),
        }
    }

    fn shared_documents(&self) -> Result<Vec<(DocumentRecord, String)>> {
        use office_core::domain::knowledge::{Page, PageFile};
        let Some(files) = self.shared_files()? else {
            return Ok(Vec::new());
        };
        let candidates = files.shared_documents()?;
        let templates = self.files.read_templates()?;
        let templates: Vec<_> = templates
            .iter()
            .map(|file| TemplateFile {
                path: &file.relative,
                markdown: file.markdown.as_deref(),
            })
            .collect();
        let own = self.files.read_knowledge_markdown()?;
        let pages: Vec<_> = own
            .iter()
            .map(|(path, markdown)| PageFile { path, markdown })
            .chain(
                candidates
                    .iter()
                    .filter(|(record, _)| {
                        matches!(record.kind, DocumentKind::Domain | DocumentKind::Runbook)
                    })
                    .map(|(record, markdown)| PageFile {
                        path: &record.path,
                        markdown,
                    }),
            )
            .collect();
        let mut accepted = Vec::new();
        for (record, markdown) in &candidates {
            if matches!(record.kind, DocumentKind::Domain | DocumentKind::Runbook)
                && Page::from_markdown(markdown)?
                    .check_template(&templates, &pages)
                    .is_err()
            {
                continue;
            }
            accepted.push((record.clone(), markdown.clone()));
        }
        Ok(accepted)
    }

    /// Revalidate shared bytes on reads: stale index rows cannot expose a newly
    /// private, malformed or redirected file before the next reindex.
    fn shared_text(&self, path: &str) -> Result<String> {
        let relative = path
            .strip_prefix("shared/")
            .ok_or_else(|| Error::UnsafePath {
                kind: "shared document",
                value: path.into(),
            })?;
        let files = self.shared_files()?.ok_or(office_core::Error::Library {
            code: "document_not_found",
            message: "shared clone is absent".into(),
        })?;
        let text = files.read_shared_text(Path::new(relative))?;
        office_core::domain::sharing::shared_document(relative, &text)?;
        Ok(text)
    }

    fn shared_companion(&self, relative: &Path) -> Result<Vec<u8>> {
        let path = relative.to_str().ok_or_else(|| Error::UnsafePath {
            kind: "shared companion",
            value: relative.display().to_string(),
        })?;
        let local = path
            .strip_prefix("shared/")
            .ok_or(office_core::Error::LibraryUnavailable)?;
        let parts: Vec<_> = local.splitn(7, '/').collect();
        if parts.len() != 7 {
            return Err(Error::UnsafePath {
                kind: "shared companion",
                value: path.into(),
            });
        }
        let directory = parts[..6].join("/");
        let name = parts[6];
        let document = if parts[2] == "sources" {
            "source.md"
        } else {
            "artifact.md"
        };
        let text = self.shared_text(&format!("shared/{directory}/{document}"))?;
        if document == "source.md" {
            office_core::domain::source::access::validate_sidecar(name)?;
        } else if !Artifact::from_markdown(&text)
            .map_err(office_core::Error::from)?
            .files
            .iter()
            .any(|file| file == name)
        {
            return Err(Error::UnsafePath {
                kind: "shared companion",
                value: path.into(),
            });
        }
        let files = self
            .shared_files()?
            .ok_or(office_core::Error::LibraryUnavailable)?;
        let local = path
            .strip_prefix("shared/")
            .ok_or(office_core::Error::LibraryUnavailable)?;
        files.read_shared_bytes(Path::new(local))
    }

    /// Decide, write, index and commit a document under one OS writer lock.
    /// Only a path `reindex` scans is accepted. Any other Markdown path is
    /// refused before the journal, the file, the index or the commit.
    /// A failure after the durable journal is written leaves a recoverable
    /// obligation; opening the store or the next write completes it.
    pub fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.write_document_locked(relative, text)
    }

    /// Resolve the current file rather than relying on an indexed approval flag.
    /// An externally deleted or renamed identity is absent, even if still indexed.
    fn current_document_locked(&self, id: &str) -> Result<Option<DocumentRecord>> {
        let Some(indexed) = self.index.get(id)? else {
            return Ok(None);
        };
        match self.files.read_document(Path::new(&indexed.path)) {
            Ok(record) => Ok((record.id == id).then_some(record)),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Resolve approval from current files, while the caller owns the writer lock.
    fn validate_sharing_locked(&self, text: &str) -> Result<()> {
        if !DocumentRecord::from_markdown(Path::new(""), text).shareable {
            return Ok(());
        }
        office_core::domain::sharing::validate_sharing(text, |id| {
            Ok(self.current_document_locked(id)?)
        })?;
        Ok(())
    }

    /// Change only the sharing field with no commit for a repeated value.
    pub fn set_shareable(&self, id: &str, shareable: bool) -> Result<DocumentRecord> {
        use office_core::domain::sharing::{self, SharingError};
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let current = self
            .current_document_locked(id)?
            .ok_or(office_core::Error::from(SharingError::DocumentNotFound))?;
        let relative = Path::new(&current.path);
        let markdown = self.files.read_text(relative)?;
        let next = sharing::with_shareable(&markdown, shareable)?;
        if shareable {
            self.validate_sharing_locked(&next)?;
        }
        if next == markdown {
            return Ok(current);
        }
        self.write_document_locked(relative, &next)
    }

    fn write_document_locked(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        self.require_initialized()?;
        self.files.validate_document_path(relative)?;
        self.validate_identity(&DocumentRecord::from_markdown(relative, text))?;
        self.validate_sharing_locked(text)?;
        let relative = relative.to_str().ok_or_else(|| Error::UnsafePath {
            kind: "document",
            value: relative.display().to_string(),
        })?;
        let pending = self.decide(WriteKind::Document, relative, text, None, None)?;
        self.finish(&pending)?;
        self.files.read_document(Path::new(relative))
    }

    /// Compare the reviewed bytes while holding the same lock as publication.
    pub fn write_reviewed_page(
        &self,
        relative: &Path,
        text: &str,
        expected: Option<&str>,
    ) -> Result<DocumentRecord> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let shared_copy;
        let own_path;
        let (relative, text, expected) = if relative.starts_with("shared") {
            let reviewed = self.shared_text(&relative.to_string_lossy())?;
            let page = office_core::domain::knowledge::Page::from_markdown(text)?;
            own_path = page.path();
            if office_core::domain::knowledge::Page::from_markdown(&reviewed)?.id != page.id
                || expected.is_none_or(|hash| sha256_hex(reviewed.as_bytes()) != hash)
                || self.index.get(&page.id)?.is_some()
                || self
                    .files
                    .root()
                    .join(&own_path)
                    .try_exists()
                    .map_err(|e| Error::io("inspect own copy", &own_path, e))?
            {
                return Err(office_core::domain::proposal::refused(
                    "proposal_revision_changed",
                    "shared page or own destination changed before publication",
                )
                .into());
            }
            shared_copy = office_core::domain::sharing::with_shareable(text, true)?;
            (Path::new(&own_path), shared_copy.as_str(), None)
        } else {
            (relative, text, expected)
        };
        self.files.validate_document_path(relative)?;
        let current = match self.files.read_text(relative) {
            Ok(text) => Some(text),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let matches = match (current.as_deref(), expected) {
            (None, None) => true,
            (Some(current), Some(hash)) => sha256_hex(current.as_bytes()) == hash,
            (None, Some(_)) | (Some(_), None) => false,
        };
        if !matches {
            return Err(office_core::domain::proposal::refused(
                "proposal_revision_changed",
                "reviewed page destination changed before publication",
            )
            .into());
        }
        self.write_document_locked(relative, text)
    }

    /// Commit a flat proposal record without indexing it as Library knowledge.
    pub fn write_proposal(&self, relative: &Path, text: &str) -> Result<()> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        self.files.validate_proposal_path(relative)?;
        let relative = relative.to_str().ok_or_else(|| Error::UnsafePath {
            kind: "proposal",
            value: relative.display().to_string(),
        })?;
        let pending = self.decide(WriteKind::Proposal, relative, text, None, None)?;
        self.finish(&pending)
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
        self.validate_sharing_locked(&text)?;
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

    /// One artifact companion file's exact bytes, by Library-relative path.
    pub fn read_artifact_file(&self, relative: &Path) -> Result<Vec<u8>> {
        self.read_ready()?;
        if relative.starts_with("shared") {
            return self.shared_companion(relative);
        }
        self.files.read_bytes(relative)
    }

    /// The shareable companions of an own source or artifact document:
    /// regular source sidecars (`vN.md`, `frames.md`, `chapters.md`,
    /// `digest.json`, `summary.md`) or the artifact's listed files. Originals,
    /// `frames/` and processing state are never returned; symlinks are refused.
    pub fn evidence_files(&self, relative: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        self.read_ready()?;
        let unsafe_path = || Error::UnsafePath {
            kind: "shared evidence",
            value: relative.display().to_string(),
        };
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(unsafe_path)?;
        let dir = relative.parent().ok_or_else(unsafe_path)?;
        let names: Vec<String> = match (relative.components().next(), name) {
            (Some(std::path::Component::Normal(area)), "source.md") if area == "sources" => {
                let physical = self.library_root().join(dir);
                self.files.validate_relative_path(dir)?;
                let mut names = Vec::new();
                let entries = std::fs::read_dir(&physical)
                    .map_err(|e| Error::io("list source", &physical, e))?;
                for entry in entries {
                    let entry = entry.map_err(|e| Error::io("list source", &physical, e))?;
                    let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                        continue;
                    };
                    let numbered = name
                        .strip_prefix('v')
                        .and_then(|n| n.strip_suffix(".md"))
                        .is_some_and(|n| {
                            !n.is_empty() && n.len() <= 10 && n.bytes().all(|b| b.is_ascii_digit())
                        });
                    if (numbered
                        || ["frames.md", "chapters.md", "digest.json", "summary.md"]
                            .contains(&name.as_str()))
                        && entry.file_type().is_ok_and(|kind| kind.is_file())
                    {
                        names.push(name);
                    }
                }
                names
            }
            (Some(std::path::Component::Normal(area)), "artifact.md") if area == "artifacts" => {
                let text = self.files.read_text(relative)?;
                Artifact::from_markdown(&text)
                    .map_err(office_core::Error::from)?
                    .files
                    .into_iter()
                    .filter(|name| {
                        !name.starts_with("original.")
                            && !name.contains('/')
                            && name != "artifact.md"
                    })
                    .collect()
            }
            _ => return Err(unsafe_path()),
        };
        let mut files = Vec::new();
        for name in names {
            let path = dir.join(&name);
            if !std::fs::symlink_metadata(self.library_root().join(&path))
                .is_ok_and(|meta| meta.is_file())
            {
                continue;
            }
            files.push((name, self.files.read_bytes(&path)?));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Read every artifact frontmatter file, without truncating series history.
    pub fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        let mut files: Vec<_> = self
            .files
            .read_artifact_markdown()?
            .into_iter()
            .map(|(path, markdown)| LibraryText {
                path,
                markdown: Some(markdown),
            })
            .collect();
        files.extend(
            self.shared_documents()?
                .into_iter()
                .filter(|(record, _)| record.kind == DocumentKind::Artifact)
                .map(|(record, markdown)| LibraryText {
                    path: record.path,
                    markdown: Some(markdown),
                }),
        );
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    fn validate_bundle(&self, pending: &PendingWrite) -> Result<()> {
        let Some(files) = &pending.bundle else {
            return Ok(());
        };
        if matches!(pending.kind, WriteKind::Bootstrap) {
            if pending.version != 6
                || pending.relative != ".office/bootstrap.json"
                || pending.original.is_some()
            {
                return Err(Error::Journal {
                    path: self.journal_path(),
                });
            }
            self.files.validate_bootstrap_path()?;
            let batches = serde_json::from_str(&pending.text).map_err(|_| Error::Journal {
                path: self.journal_path(),
            })?;
            office_core::domain::bootstrap::validate_batches(&batches)?;
            let mut seen = std::collections::BTreeSet::new();
            for file in files {
                let relative = Path::new(&file.path);
                if !file.path.starts_with("sources/")
                    || !file.path.ends_with("/source.md")
                    || !seen.insert(&file.path)
                {
                    return Err(Error::Journal {
                        path: self.journal_path(),
                    });
                }
                self.files.validate_document_path(relative)?;
                let markdown = std::str::from_utf8(&file.bytes).map_err(|_| Error::Journal {
                    path: self.journal_path(),
                })?;
                self.validate_identity(&DocumentRecord::from_markdown(relative, markdown))?;
            }
            return Ok(());
        }
        if matches!(pending.kind, WriteKind::Video | WriteKind::SourceProcessing) {
            if pending.version
                != if matches!(pending.kind, WriteKind::Video) {
                    5
                } else {
                    7
                }
                || pending.original.is_some()
                || !pending.relative.starts_with("sources/")
                || !pending.relative.ends_with("/source.md")
            {
                return Err(Error::Journal {
                    path: self.journal_path(),
                });
            }
            self.files
                .validate_document_path(Path::new(&pending.relative))?;
            if matches!(pending.kind, WriteKind::Video) {
                office_core::domain::source::video::state(&pending.text)
                    .map_err(Error::from)?
                    .ok_or_else(|| Error::Journal {
                        path: self.journal_path(),
                    })?;
            }
            let parent = Path::new(&pending.relative)
                .parent()
                .ok_or_else(|| Error::Journal {
                    path: self.journal_path(),
                })?;
            let mut seen = std::collections::BTreeSet::new();
            for file in files {
                let image = file
                    .path
                    .strip_prefix("frames/")
                    .and_then(|s| s.strip_suffix(".png"))
                    .is_some_and(|s| {
                        (4..=6).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit())
                    });
                let processing = matches!(pending.kind, WriteKind::SourceProcessing)
                    && office_core::domain::source::processing_file_name(&file.path);
                let allowed = if matches!(pending.kind, WriteKind::SourceProcessing) {
                    processing
                } else {
                    file.path == "frames.md" || image
                };
                if !allowed || !seen.insert(&file.path) {
                    return Err(Error::Journal {
                        path: self.journal_path(),
                    });
                }
                self.files
                    .validate_relative_path(&parent.join(&file.path))?;
                if file.path.starts_with('v') && file.path.ends_with(".md") {
                    match self.files.read_bytes(&parent.join(&file.path)) {
                        Ok(bytes) if bytes != file.bytes => {
                            return Err(Error::Journal {
                                path: self.journal_path(),
                            });
                        }
                        Ok(_) => {}
                        Err(Error::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            return Ok(());
        }
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
            version: if matches!(kind, WriteKind::Proposal) {
                8
            } else if matches!(kind, WriteKind::SourceProcessing) {
                7
            } else if matches!(kind, WriteKind::Bootstrap) {
                6
            } else if matches!(kind, WriteKind::Video) {
                5
            } else if bundle.is_some() {
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
        // The store root is canonical, so compare the journal root by its
        // physical path: a root spelled through a symlinked ancestor (macOS
        // `/var` -> `/private/var`) is the same Library. A root that no
        // longer resolves cannot be this Library and is refused.
        let same_root = pending
            .root
            .canonicalize()
            .is_ok_and(|root| root == self.library_root());
        if !(1..=LATEST_PENDING_WRITE_VERSION).contains(&pending.version)
            || !same_root
            || pending.token.is_empty()
            || !pending
                .token
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::Journal { path });
        }
        match pending.kind {
            WriteKind::Proposal => {
                self.require_initialized()?;
                self.files
                    .validate_proposal_path(Path::new(&pending.relative))?;
            }
            WriteKind::Document => {
                self.require_initialized()?;
                self.files
                    .validate_document_path(Path::new(&pending.relative))?;
                self.validate_identity(&DocumentRecord::from_markdown(
                    Path::new(&pending.relative),
                    &pending.text,
                ))?;
            }
            WriteKind::Artifact | WriteKind::Video | WriteKind::SourceProcessing => {
                self.require_initialized()?;
                self.validate_bundle(&pending)?;
                self.validate_identity(&DocumentRecord::from_markdown(
                    Path::new(&pending.relative),
                    &pending.text,
                ))?;
            }
            WriteKind::Bootstrap => {
                self.require_initialized()?;
                self.validate_bundle(&pending)?;
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
            WriteKind::Proposal => {
                self.files
                    .write_proposal(Path::new(&pending.relative), &pending.text)?;
            }
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
            WriteKind::Video | WriteKind::SourceProcessing => {
                self.validate_bundle(pending)?;
                let parent =
                    Path::new(&pending.relative)
                        .parent()
                        .ok_or_else(|| Error::Journal {
                            path: self.journal_path(),
                        })?;
                for file in pending.bundle.as_deref().unwrap_or_default() {
                    let relative = parent.join(&file.path);
                    self.files.validate_relative_path(&relative)?;
                    let path = self.library_root().join(&relative);
                    if let Some(parent) = path.parent() {
                        LibraryFs::create_private_dirs(parent)?;
                    }
                    if file.path.starts_with('v') && file.path.ends_with(".md") {
                        self.files.write_original(&relative, &file.bytes)?;
                    } else {
                        write_atomic(&path, &file.bytes)?;
                    }
                }
                self.files
                    .write_document(Path::new(&pending.relative), &pending.text)?;
                self.index
                    .upsert(&self.files.read_document(Path::new(&pending.relative))?)?;
            }
            WriteKind::Bootstrap => {
                self.validate_bundle(pending)?;
                for file in pending.bundle.as_deref().unwrap_or_default() {
                    let text = std::str::from_utf8(&file.bytes).map_err(|_| Error::Journal {
                        path: self.journal_path(),
                    })?;
                    self.files.write_document(Path::new(&file.path), text)?;
                    self.index
                        .upsert(&self.files.read_document(Path::new(&file.path))?)?;
                }
                self.files.write_bootstrap(&pending.text)?;
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
                    .filter(|file| {
                        !matches!(pending.kind, WriteKind::Video | WriteKind::SourceProcessing)
                            || !file.path.starts_with("frames/")
                    })
                    .map(|file| {
                        if matches!(pending.kind, WriteKind::Bootstrap) {
                            file.path.clone()
                        } else {
                            parent.join(&file.path).to_string_lossy().to_string()
                        }
                    }),
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
        if relative.starts_with("shared") {
            let text = self.shared_text(&relative.to_string_lossy())?;
            return Ok(DocumentRecord::from_markdown(relative, &text));
        }
        self.files.validate_document_path(relative)?;
        self.files.read_document(relative)
    }

    /// One indexed document by id, if present. Does not wait for the writer lock.
    pub fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        self.read_ready()?;
        let record = if let Some((member, local)) = id.split_once('/') {
            if self
                .settings()?
                .shared
                .as_ref()
                .is_some_and(|settings| settings.member == member)
            {
                self.index.get(local)?
            } else {
                let record = if office_core::domain::knowledge::page_kind_for_id(local).is_some() {
                    self.index.get_shared(local)?
                } else {
                    self.index.get_shared(id)?
                };
                record.filter(|record| record.shared_by.as_deref() == Some(member))
            }
        } else {
            self.index.get(id)?.or(self.index.get_shared(id)?)
        };
        match record {
            Some(record) if record.layer == LibraryLayer::Shared => {
                match self.shared_text(&record.path) {
                    Ok(text) => {
                        let mut current =
                            DocumentRecord::from_markdown(Path::new(&record.path), &text);
                        if current.kind == DocumentKind::Source
                            && let Some(files) = self.shared_files()?
                        {
                            let relative = Path::new(&record.path)
                                .strip_prefix("shared")
                                .map_err(|_| office_core::Error::LibraryUnavailable)?;
                            files.shared_source_body(relative, &mut current)?;
                        }
                        Ok(Some(current))
                    }
                    Err(Error::Settings(_)) => Ok(None),
                    Err(Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
            own => Ok(own),
        }
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
        let mut files: Vec<_> = self
            .files
            .read_knowledge_markdown()?
            .into_iter()
            .map(|(path, markdown)| LibraryText {
                path,
                markdown: Some(markdown),
            })
            .collect();
        files.extend(
            self.shared_documents()?
                .into_iter()
                .filter(|(record, _)| {
                    matches!(record.kind, DocumentKind::Domain | DocumentKind::Runbook)
                })
                .map(|(record, markdown)| LibraryText {
                    path: record.path,
                    markdown: Some(markdown),
                }),
        );
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Every current source file, in path order, without a search limit.
    /// Reads full Markdown from disk without waiting for the writer lock.
    pub fn source_documents(&self) -> Result<Vec<LibraryText>> {
        self.read_ready()?;
        Ok(self
            .files
            .read_source_markdown()?
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
        if relative.starts_with("shared") {
            self.shared_text(&relative.to_string_lossy())?;
            let files = self
                .shared_files()?
                .ok_or(office_core::Error::LibraryUnavailable)?;
            let local = relative
                .strip_prefix("shared")
                .map_err(|_| office_core::Error::LibraryUnavailable)?;
            return Git::new(files.root(), &self.state).history(&local.to_string_lossy());
        }
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
            .map(normalize_upload_extension)
            .is_some_and(|ext| convert::is_document(&ext))
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

    fn load_session(
        &self,
        raw: &str,
        id: &str,
    ) -> office_core::Result<office_core::domain::source::conversation::Session> {
        use office_core::domain::source::{conversation, video::refusal};
        use std::io::Read;
        let canonical = std::fs::canonicalize(raw).map_err(|_| {
            refusal(
                "session_transcript_unavailable",
                "session transcript is unavailable",
            )
        })?;
        if !self
            .upload_roots
            .iter()
            .any(|root| std::fs::canonicalize(root).is_ok_and(|root| canonical.starts_with(root)))
        {
            return Err(refusal(
                "upload_outside_allowed_roots",
                "stage the transcript inside the granted workspace or Library",
            ));
        }
        if !std::fs::metadata(&canonical).is_ok_and(|m| m.is_file()) {
            return Err(refusal(
                "session_transcript_unavailable",
                "transcript must be a regular file",
            ));
        }
        let file = std::fs::File::open(&canonical).map_err(|_| {
            refusal(
                "session_transcript_unavailable",
                "session transcript is unavailable",
            )
        })?;
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return Err(refusal(
                "session_transcript_unavailable",
                "transcript must be a regular file",
            ));
        }
        // Verify the opened descriptor still names the physically checked file.
        let current = std::fs::symlink_metadata(&canonical).map_err(|_| {
            refusal(
                "session_transcript_unavailable",
                "transcript changed while opening",
            )
        })?;
        let opened = file.metadata().map_err(|_| {
            refusal(
                "session_transcript_unavailable",
                "could not inspect transcript",
            )
        })?;
        let mut same_file = current.is_file()
            && std::fs::canonicalize(&canonical).is_ok_and(|path| path == canonical);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            same_file &= current.dev() == opened.dev() && current.ino() == opened.ino();
        }
        #[cfg(not(unix))]
        {
            let _ = opened;
        }
        if !same_file {
            return Err(refusal(
                "session_transcript_unavailable",
                "transcript changed while opening",
            ));
        }
        let mut bytes = Vec::new();
        file.take(conversation::INPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                refusal(
                    "session_transcript_unavailable",
                    "could not read session transcript",
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > conversation::INPUT_LIMIT {
            return Err(refusal(
                "session_transcript_too_large",
                "session transcript exceeds 8 MiB",
            ));
        }
        let raw = std::str::from_utf8(&bytes).map_err(|_| {
            refusal(
                "invalid_session_transcript",
                "session transcript must be UTF-8",
            )
        })?;
        let mut session = conversation::parse(raw, id)?;
        let settings = self.settings()?;
        let existing_private = self.source_views()?.iter().any(|(_, text)| {
            office_core::domain::metrics::metadata(text).is_ok_and(|meta| {
                meta.get("session_id").and_then(serde_json::Value::as_str) == Some(id)
                    && parse_source_meta(text).sensitivity == "private"
            })
        });
        if session.scrubbed
            || existing_private
            || settings.sensitivity.get("conversation") == Some(&Sensitivity::Private)
        {
            session.sensitivity = "private".into();
        }
        Ok(session)
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
            upload_ext =
                normalize_upload_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
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
            transcript_body = Some(vtt_to_body(&text)?);
        }
        if transcript_body.is_none() && upload_ext == "vtt" {
            let bytes = upload_bytes.as_deref().unwrap_or_default();
            let text = std::str::from_utf8(bytes).map_err(|_| {
                office_core::Error::InvalidArgument("upload must be UTF-8 WebVTT".into())
            })?;
            transcript_body = Some(vtt_to_body(text)?);
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
        if request.event.is_none()
            && let Some(sha_value) = &sha
        {
            for (path, text) in &existing {
                let meta = parse_source_meta(text);
                let text_sha = sha256_hex(normalize_text(&source_body(text)).as_bytes());
                if meta.sha256.as_ref() == Some(sha_value) || &text_sha == sha_value {
                    if let Some(markdown) =
                        office_core::domain::source::merge_source_tags(text, &request.tags)?
                    {
                        let pending = self
                            .decide(WriteKind::Document, path, &markdown, None, None)
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
        let month = format!("sources/{}/{yyyy}/{mm}", kind.as_str());
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
            event: request.event.as_ref(),
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
        if let Some(record) = self.get(id)? {
            if record.kind != DocumentKind::Source {
                return Err(office_core::Error::Library {
                    code: "source_not_found",
                    message: "id is not a source".into(),
                });
            }
            let text = self.read_text(Path::new(&record.path))?;
            let meta = parse_source_meta(&text);
            return Ok(SourceView {
                id: record.id,
                kind: meta.kind,
                title: meta.title,
                path: record.path,
                body: source_body(&text).trim_end_matches('\n').to_string(),
                sensitivity: record.sensitivity,
                sha256: meta.sha256,
                bytes: meta.bytes,
                media_type: meta.media_type,
                version: meta.version,
            });
        }
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
        if relative.starts_with("shared") {
            return self.shared_text(&relative.to_string_lossy());
        }
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

fn dir_is_empty(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

impl office_core::ports::Library for LibraryStore {
    fn set_shareable(&self, id: &str, shareable: bool) -> office_core::Result<DocumentRecord> {
        Ok(Self::set_shareable(self, id, shareable)?)
    }
    fn write_reviewed_page(
        &self,
        relative: &Path,
        text: &str,
        expected: Option<&str>,
    ) -> office_core::Result<DocumentRecord> {
        Ok(Self::write_reviewed_page(self, relative, text, expected)?)
    }
    fn write_proposal(&self, relative: &Path, text: &str) -> office_core::Result<()> {
        Ok(Self::write_proposal(self, relative, text)?)
    }

    fn bootstrap_snapshot(&self) -> office_core::Result<office_core::domain::bootstrap::Snapshot> {
        use office_core::domain::bootstrap::{Snapshot, Source, digest, validate_batches};
        self.read_ready()?;
        let mut snapshot = Snapshot::default();
        for file in self.source_documents()? {
            let markdown = file.markdown.ok_or_else(|| {
                office_core::domain::source::video::refusal(
                    "invalid_bootstrap_data",
                    "source is unreadable",
                )
            })?;
            let path = Path::new(&file.path).with_file_name("digest.json");
            let value = self
                .files
                .optional_bootstrap_json(&path)?
                .map(|text| {
                    serde_json::from_str::<serde_json::Value>(&text).map_err(|_| {
                        office_core::domain::source::video::refusal(
                            "invalid_bootstrap_data",
                            "digest.json is malformed",
                        )
                    })
                })
                .transpose()?;
            if let Some(value) = &value {
                digest(value)?;
            }
            snapshot.sources.push(Source {
                path: file.path,
                markdown,
                digest: value,
            });
        }
        let mut seen_pages = std::collections::BTreeSet::new();
        for page in self.knowledge_pages()? {
            let markdown = page.markdown.ok_or_else(|| {
                office_core::domain::source::video::refusal(
                    "invalid_bootstrap_data",
                    "page is unreadable",
                )
            })?;
            let id = DocumentRecord::from_markdown(Path::new(&page.path), &markdown).id;
            if seen_pages.insert(id) {
                snapshot.pages.insert(page.path, markdown);
            }
        }
        if let Some(text) = self
            .files
            .optional_bootstrap_json(Path::new(".office/bootstrap.json"))?
        {
            snapshot.batches = serde_json::from_str(&text).map_err(|_| {
                office_core::domain::source::video::refusal(
                    "invalid_bootstrap_data",
                    "bootstrap ledger is malformed",
                )
            })?;
            validate_batches(&snapshot.batches)?;
        }
        Ok(snapshot)
    }

    fn mutate_batch(
        &self,
        revision: &str,
        mutation: office_core::domain::bootstrap::Mutation,
    ) -> office_core::Result<office_core::domain::bootstrap::Batch> {
        use office_core::domain::{bootstrap, source::video::refusal};
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let snapshot = <Self as office_core::ports::Library>::bootstrap_snapshot(self)?;
        let settings = self.settings()?;
        if snapshot.revision(&settings)? != revision {
            return Err(refusal(
                "plan_changed",
                "Library changed during batch planning; preview again",
            ));
        }
        let write = bootstrap::apply(&snapshot, &settings, mutation)?;
        if write.batches == snapshot.batches && write.sources.is_empty() {
            return Ok(write.batch);
        }
        let text = serde_json::to_string_pretty(&write.batches)
            .map_err(|_| refusal("invalid_bootstrap_data", "cannot encode bootstrap ledger"))?;
        let files = write
            .sources
            .into_iter()
            .map(|(path, text)| ArtifactFile {
                path,
                bytes: text.into_bytes(),
            })
            .collect();
        let pending = self.decide(
            WriteKind::Bootstrap,
            ".office/bootstrap.json",
            &text,
            None,
            Some(files),
        )?;
        self.finish(&pending)?;
        Ok(write.batch)
    }

    fn read_video(
        &self,
        id: &str,
    ) -> office_core::Result<office_core::domain::source::video::VideoSource> {
        let source = <Self as office_core::ports::Library>::read_media(self, id)?;
        if parse_source_meta(&source.markdown).kind != "video" {
            return Err(office_core::domain::source::video::refusal(
                "not_a_video_source",
                "source is not a video",
            ));
        }
        Ok(source)
    }
    fn source_media(
        &self,
        id: &str,
        selection: &office_core::domain::source::access::MediaSelection,
    ) -> office_core::Result<office_core::domain::source::access::SourceMedia> {
        use office_core::domain::source::{access, video::refusal};
        use std::io::Read;
        // Keep this fresh descriptor alive until after payload access. A normal
        // source cannot be upgraded to private between policy and bytes.
        let path = self.state.join("library.writer.lock");
        let guard =
            std::fs::File::open(&path).map_err(|e| Error::io("open media read guard", &path, e))?;
        match guard.try_lock_shared() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(refusal(
                    "library_writer_busy",
                    "source media is unavailable during a Library write; retry the read",
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(Error::io("lock source media read", &path, e).into());
            }
        }
        let source = self.read_source(id)?;
        let markdown = self.files.read_text(Path::new(&source.path))?;
        let (name, media_type) = access::media_file(&markdown, selection, &self.settings()?)?;
        let relative = Path::new(&source.path).with_file_name(&name);
        self.files.validate_relative_path(&relative)?;
        let path = self.library_root().join(relative);
        // Refuse special files before open: opening a FIFO could otherwise
        // block before the descriptor's own metadata can be checked.
        if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
            return Err(refusal(
                "source_media_unavailable",
                "source media is not an available regular file",
            ));
        }
        let file = std::fs::File::open(&path).map_err(|_| {
            refusal(
                "source_media_unavailable",
                "source media file is unavailable",
            )
        })?;
        let metadata = file
            .metadata()
            .map_err(|e| Error::io("inspect source media", &path, e))?;
        if !metadata.is_file() {
            return Err(refusal(
                "source_media_unavailable",
                "source media is not a regular file",
            ));
        }
        let limit = u64::try_from(access::MEDIA_LIMIT)
            .map_err(|_| refusal("invalid_source", "media limit overflow"))?;
        if metadata.len() > limit {
            return Ok(access::SourceMedia {
                name,
                media_type,
                bytes: metadata.len(),
                content: None,
            });
        }
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::io("read bounded source media", &path, e))?;
        let size = u64::try_from(bytes.len())
            .map_err(|_| refusal("invalid_source", "media size overflow"))?;
        let content = if size > limit {
            None
        } else {
            if matches!(selection, access::MediaSelection::Original)
                && parse_source_meta(&markdown).sha256.as_deref()
                    != Some(sha256_hex(&bytes).as_str())
            {
                return Err(refusal(
                    "plan_changed",
                    "original no longer matches its recorded digest",
                ));
            }
            Some(bytes)
        };
        Ok(access::SourceMedia {
            name,
            media_type,
            bytes: size,
            content,
        })
    }
    fn set_source_sensitivity(
        &self,
        id: &str,
        sensitivity: &str,
    ) -> office_core::Result<office_core::ports::SensitivityOutcome> {
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let source = self.read_source(id)?;
        let markdown = self.files.read_text(Path::new(&source.path))?;
        let unshared =
            sensitivity == "private" && office_core::domain::sharing::shareable(&markdown)?;
        let next = office_core::domain::source::access::with_sensitivity(&markdown, sensitivity)?;
        if next != markdown {
            self.write_document_locked(Path::new(&source.path), &next)?;
        }
        Ok(office_core::ports::SensitivityOutcome {
            source: self.read_source(id)?,
            unshared,
        })
    }
    fn read_media(
        &self,
        id: &str,
    ) -> office_core::Result<office_core::domain::source::video::VideoSource> {
        use office_core::domain::source::video::{VideoSource, refusal};
        let view = self.read_source(id)?;
        if view.kind != "video" && view.kind != "meeting" {
            return Err(refusal(
                "invalid_source_kind",
                "transcription needs a video or meeting source",
            ));
        }
        let markdown = self.files.read_text(Path::new(&view.path))?;
        let meta = parse_source_meta(&markdown);
        let name = meta
            .original_file
            .as_deref()
            .filter(|name| name.starts_with("original."))
            .ok_or_else(|| refusal("original_unavailable", "video original is unavailable"))?;
        let relative = Path::new(&view.path)
            .parent()
            .ok_or_else(|| refusal("invalid_video_source", "source path has no parent"))?
            .join(name);
        self.files.validate_relative_path(&relative)?;
        let bytes = std::fs::read(self.library_root().join(&relative))
            .map_err(|_| refusal("original_unavailable", "video original is unavailable"))?;
        if meta.sha256.as_deref() != Some(sha256_hex(&bytes).as_str()) {
            return Err(refusal(
                "plan_changed",
                "original no longer matches its recorded digest",
            ));
        }
        Ok(VideoSource {
            path: view.path,
            markdown,
            bytes,
        })
    }
    fn source_sidecar(&self, id: &str, name: &str) -> office_core::Result<Option<String>> {
        office_core::domain::source::access::validate_sidecar(name)?;
        let source = self.read_source(id)?;
        let relative = Path::new(&source.path).with_file_name(name);
        if relative.starts_with("shared") {
            return match self.shared_companion(&relative) {
                Ok(bytes) => {
                    String::from_utf8(bytes)
                        .map(Some)
                        .map_err(|_| office_core::Error::Library {
                            code: "library_document_unreadable",
                            message: "shared sidecar is not UTF-8".into(),
                        })
                }
                Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                    Ok(None)
                }
                Err(error) => Err(error.into()),
            };
        }
        self.files.validate_relative_path(&relative)?;
        match std::fs::read_to_string(self.library_root().join(relative)) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("read source sidecar", self.library_root(), error).into()),
        }
    }
    fn write_source_processing(
        &self,
        id: &str,
        revision: &str,
        body: Option<&str>,
        derived: &office_core::ports::DerivedBy,
        files: &[(String, Vec<u8>)],
    ) -> office_core::Result<SourceView> {
        use office_core::domain::source::{processing_markdown, video::refusal};
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let source = self.read_source(id)?;
        let markdown = self.files.read_text(Path::new(&source.path))?;
        if sha256_hex(markdown.as_bytes()) != revision {
            return Err(refusal("plan_changed", "source changed during processing"));
        }
        let (text, snapshot) = processing_markdown(&markdown, body, derived)?;
        let mut bundle: Vec<ArtifactFile> = files
            .iter()
            .map(|(path, bytes)| ArtifactFile {
                path: path.clone(),
                bytes: bytes.clone(),
            })
            .collect();
        if let Some(path) = snapshot {
            bundle.push(ArtifactFile {
                path,
                bytes: text.as_bytes().to_vec(),
            });
        }
        let pending = self.decide(
            WriteKind::SourceProcessing,
            &source.path,
            &text,
            None,
            Some(bundle),
        )?;
        self.finish(&pending)?;
        self.read_source(id)
    }
    fn mutate_video(
        &self,
        id: &str,
        revision: &str,
        mutation: office_core::domain::source::video::VideoMutation,
    ) -> office_core::Result<office_core::domain::source::video::VideoState> {
        use office_core::domain::source::video::{apply, refusal};
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        let source = <Self as office_core::ports::Library>::read_video(self, id)?;
        if sha256_hex(source.markdown.as_bytes()) != revision {
            return Err(refusal(
                "plan_changed",
                "video changed during processing; read its current plan",
            ));
        }
        let write = apply(&source, &self.settings()?, mutation)?;
        if write.markdown == source.markdown && write.files.is_empty() {
            return Ok(write.state);
        }
        let files = write
            .files
            .into_iter()
            .map(|(path, bytes)| ArtifactFile { path, bytes })
            .collect();
        let pending = self.decide(
            WriteKind::Video,
            &source.path,
            &write.markdown,
            None,
            Some(files),
        )?;
        self.finish(&pending)?;
        Ok(write.state)
    }

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
    fn read_artifact_file(&self, relative: &Path) -> office_core::Result<Vec<u8>> {
        Ok(Self::read_artifact_file(self, relative)?)
    }
    fn evidence_files(&self, relative: &Path) -> office_core::Result<Vec<(String, Vec<u8>)>> {
        Ok(Self::evidence_files(self, relative)?)
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
    fn source_documents(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(Self::source_documents(self)?)
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
    fn session_transcript(
        &self,
        path: &str,
        session_id: &str,
    ) -> office_core::Result<office_core::domain::source::conversation::Session> {
        use office_core::domain::source::video::refusal;
        // A privacy change cannot race preview disclosure. Read-only: never create a lock.
        let guard_path = self.state.join("library.writer.lock");
        let guard = std::fs::File::open(&guard_path)
            .map_err(|e| Error::io("open transcript read guard", &guard_path, e))?;
        match guard.try_lock_shared() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(refusal(
                    "library_writer_busy",
                    "transcript preview unavailable during a Library write; retry",
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(Error::io("lock transcript read", &guard_path, e).into());
            }
        }
        self.require_initialized()?;
        let mut session = self.load_session(path, session_id)?;
        if session.sensitivity == "private" {
            session.lines.clear();
        }
        Ok(session)
    }
    fn capture_session(
        &self,
        request: &office_core::domain::source::conversation::Request,
        captured_at: &str,
    ) -> office_core::Result<SourceOutcome> {
        use office_core::domain::source::{conversation, video::refusal};
        let _lock = WriterLock::acquire(&self.state)?;
        self.recover()?;
        self.require_initialized()?;
        let mut session = self.load_session(&request.path, &request.session_id)?;
        let entries = conversation::entries(&session, request)?;
        let existing = self.source_views()?;
        let mut matches = Vec::new();
        for (path, text) in &existing {
            conversation::validate_provenance(text)?;
            let meta = office_core::domain::metrics::metadata(text)?;
            if meta.get("session_id").and_then(serde_json::Value::as_str)
                == Some(&request.session_id)
            {
                matches.push((path, text, meta));
            }
        }
        if matches.len() > 1 {
            return Err(refusal(
                "session_source_conflict",
                "multiple sources have this session id; reconcile them before capture",
            ));
        }
        let (id, path, base, changed) = if let Some((path, text, meta)) = matches.first() {
            if parse_source_meta(text).kind != "conversation"
                || meta
                    .get("orchestrator")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|p| p != session.orchestrator)
            {
                return Err(refusal(
                    "session_source_conflict",
                    "session id belongs to incompatible source provenance",
                ));
            }
            let id = parse_source_meta(text).id;
            if meta
                .get("transcript_sha256")
                .and_then(serde_json::Value::as_str)
                == Some(&session.transcript_sha256)
            {
                let view = self.read_source(&id)?;
                return Ok(SourceOutcome {
                    id,
                    kind: "conversation".into(),
                    title: view.title,
                    path: view.path,
                    sensitivity: view.sensitivity,
                    deduplicated: true,
                    version: view.version,
                    ..SourceOutcome::default()
                });
            }
            (id, (*path).clone(), (*text).clone(), true)
        } else {
            let at = &session.lines[0].timestamp;
            let date = at
                .get(..10)
                .ok_or_else(|| refusal("invalid_session_transcript", "invalid session date"))?
                .to_string();
            let base_id = source_id_base(
                &date,
                &format!(
                    "session-{}",
                    &sha256_hex(request.session_id.as_bytes())[..12]
                ),
            );
            let id = unique_source_id(&base_id, |candidate| {
                existing
                    .iter()
                    .any(|(_, text)| parse_source_meta(text).id == candidate)
            });
            let path = format!(
                "sources/conversation/{}/{}/{id}/source.md",
                &date[..4],
                &date[5..7]
            );
            let text = render_source(&RenderSource {
                id: &id,
                kind: SourceKind::Conversation,
                title: "Conversation session",
                captured_at,
                captured_by: "transcript-extractor",
                origin: None,
                occurred_at: Some(at),
                participants: &[],
                event: None,
                tags: &[],
                sensitivity: Sensitivity::parse(&session.sensitivity).unwrap_or_default(),
                original: None,
                derived_transcription: None,
                retention_days: None,
                body: "",
            });
            (id, path, text, false)
        };
        let scrubbed_base = conversation::scrub_source(&base)?;
        if scrubbed_base != base {
            session.sensitivity = "private".into();
        }
        let base = scrubbed_base;
        let text = conversation::merge(&base, &session, &entries, &session.sensitivity, changed)?;
        self.validate_identity(&DocumentRecord::from_markdown(Path::new(&path), &text))?;
        let bundle = if changed {
            vec![ArtifactFile {
                path: format!("v{}.md", parse_source_meta(&base).version),
                bytes: base.into_bytes(),
            }]
        } else {
            vec![]
        };
        let pending = self.decide(
            WriteKind::SourceProcessing,
            &path,
            &text,
            None,
            Some(bundle),
        )?;
        self.finish(&pending)?;
        Ok(SourceOutcome {
            id,
            kind: "conversation".into(),
            title: parse_source_meta(&text).title,
            path,
            sensitivity: session.sensitivity,
            deduplicated: false,
            version: parse_source_meta(&text).version,
            ..SourceOutcome::default()
        })
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
