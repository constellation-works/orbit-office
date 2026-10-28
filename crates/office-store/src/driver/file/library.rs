//! Reading and writing Library documents on disk, and scanning the indexed
//! layers for a reindex.
//!
//! The pure frontmatter reader lives with `DocumentRecord` in office-core;
//! files remain the source of truth for fields the index does not consume.

use std::path::{Path, PathBuf};

use crate::contracts::DocumentRecord;
use crate::error::{Error, Result};
use office_common::fs::atomic::{write_atomic, write_atomic_new};
use office_common::fs::path_safety::validate_path_stem;
use office_core::domain::template::BUNDLED;

/// A template file under `.office/templates/`, read without following a symlink.
pub(crate) struct TemplateOnDisk {
    /// Library-relative path, forward slashes.
    pub(crate) relative: String,
    /// Markdown, or `None` when the path is a symlink or could not be read.
    pub(crate) markdown: Option<String>,
}

/// Directory layout created under a Library root.
pub(crate) const LAYOUT_DIRS: &[&str] = &[
    "sources",
    "knowledge/domain",
    "knowledge/runbooks",
    "artifacts",
    "proposals",
    ".office",
];

/// Layers `reindex` rebuilds, in scan order. An empty stem list means every
/// `*.md` file; otherwise only those stems (`source.md` beside `original.*`
/// and `v2.md`, `artifact.md` beside its companion files). Document writes
/// accept only paths this table covers, so a successful write survives the
/// next rebuild.
const INDEXED_DOCUMENT_LAYERS: &[(&str, &[&str])] = &[
    ("sources", &["source"]),
    ("knowledge", &[]),
    ("artifacts", &["artifact"]),
];

const INDEXED_DOCUMENT_KIND: &str = concat!(
    "indexed document (",
    "sources/**/source.md, knowledge/**/*.md, artifacts/**/artifact.md",
    ")"
);

/// The file driver over one Library root.
pub(crate) struct LibraryFs {
    root: PathBuf,
}

impl LibraryFs {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The Library root this driver reads and writes.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Create the Library layout directories (idempotent).
    pub(crate) fn ensure_layout(&self) -> Result<()> {
        for dir in LAYOUT_DIRS {
            let mut path = self.root.clone();
            for component in Path::new(dir).components() {
                path.push(component);
                refuse_symlink(&path)?;
            }
            Self::create_private_dirs(&path)?;
        }
        Ok(())
    }

    /// Create only missing directories with owner-only permissions.
    pub(crate) fn create_private_dirs(path: &Path) -> Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|e| Error::io("create Library directory", path, e))
    }

    /// Read the fixed root ignore file, refusing a symlink before use.
    pub(crate) fn gitignore_text(&self) -> Result<Option<String>> {
        let path = self.root.join(".gitignore");
        refuse_symlink(&path)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("read gitignore", path, error)),
        }
    }
    /// Read internal settings without following a symlink out of the Library.
    pub(crate) fn settings_text(&self) -> Result<Option<String>> {
        self.validate_settings_path()?;
        let path = self.root.join(".office/config.yaml");
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io("read settings", path, e)),
        }
    }

    /// The fixed internal path permits `.office`, unlike caller-supplied
    /// document components, and never follows a symlink in either component.
    pub(crate) fn validate_settings_path(&self) -> Result<()> {
        refuse_symlink(&self.root.join(".office"))?;
        refuse_symlink(&self.root.join(".office/config.yaml"))
    }

    /// Replace only the internal config, with the same path containment and
    /// atomic private-file writer used for documents.
    pub(crate) fn write_settings(&self, text: &str) -> Result<()> {
        self.validate_settings_path()?;
        Ok(write_atomic(
            &self.root.join(".office/config.yaml"),
            text.as_bytes(),
        )?)
    }

    /// Read a digest sidecar or the fixed bootstrap ledger without following links.
    /// Missing is distinct from unreadable or invalid JSON.
    pub(crate) fn optional_bootstrap_json(&self, relative: &Path) -> Result<Option<String>> {
        if relative == Path::new(".office/bootstrap.json") {
            refuse_symlink(&self.root.join(".office"))?;
            refuse_symlink(&self.root.join(relative))?;
        } else {
            self.validate_relative_path(relative)?;
            if !relative.starts_with("sources")
                || relative.file_name().is_none_or(|n| n != "digest.json")
            {
                return Err(Error::UnsafePath {
                    kind: "bootstrap JSON",
                    value: relative.display().to_string(),
                });
            }
        }
        let path = self.root.join(relative);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io("read bootstrap JSON", &path, error)),
        }
    }

    /// Validate the fixed internal ledger path before the durable write decision.
    pub(crate) fn validate_bootstrap_path(&self) -> Result<()> {
        refuse_symlink(&self.root.join(".office"))?;
        refuse_symlink(&self.root.join(".office/bootstrap.json"))
    }

    /// Atomically replace only the fixed internal bootstrap ledger.
    pub(crate) fn write_bootstrap(&self, text: &str) -> Result<()> {
        self.validate_bootstrap_path()?;
        Ok(write_atomic(
            &self.root.join(".office/bootstrap.json"),
            text.as_bytes(),
        )?)
    }

    /// Read one Library file as exact bytes, with relative-path containment.
    /// No extension or UTF-8 requirement, so artifact companions of any
    /// kind can be read; symlink components are refused as elsewhere.
    pub(crate) fn read_bytes(&self, relative: &Path) -> Result<Vec<u8>> {
        self.validate_relative_path(relative)?;
        let path = self.root.join(relative);
        std::fs::read(&path).map_err(|e| Error::io("read", &path, e))
    }

    /// Read one Library file as text, with relative-path containment.
    pub(crate) fn read_text(&self, relative: &Path) -> Result<String> {
        self.validate_relative_path(relative)?;
        let path = self.root.join(relative);
        std::fs::read_to_string(&path).map_err(|e| Error::io("read", &path, e))
    }

    /// Publish an original without replacement. Only identical existing bytes
    /// are an idempotent replay; a conflicting entry keeps recovery pending.
    pub(crate) fn write_original(&self, relative: &Path, bytes: &[u8]) -> Result<()> {
        self.validate_relative_path(relative)?;
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            Self::create_private_dirs(parent)?;
        }
        match write_atomic_new(&path, bytes) {
            Ok(()) => {}
            Err(office_common::Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                self.validate_relative_path(relative)?;
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|e| Error::io("inspect existing original", &path, e))?;
                if !metadata.is_file() {
                    return Err(Error::io(
                        "publish original over occupied path",
                        path,
                        source,
                    ));
                }
                let existing = std::fs::read(&path)
                    .map_err(|e| Error::io("read existing original", &path, e))?;
                if existing != bytes {
                    return Err(Error::io(
                        "publish original without replacing existing data",
                        path,
                        source,
                    ));
                }
                // Also covers a crash immediately after publication but before
                // its parent directory was synced.
                std::fs::File::open(&path)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| Error::io("sync original", &path, e))?;
            }
            Err(error) => return Err(error.into()),
        }
        // Persist newly created ancestors too, before the journal is removed.
        let mut parent = path.parent();
        while let Some(dir) = parent {
            std::fs::File::open(dir)
                .and_then(|file| file.sync_all())
                .map_err(|e| Error::io("sync original directory", dir, e))?;
            if dir == self.root {
                break;
            }
            parent = dir.parent();
        }
        Ok(())
    }

    /// Stage a complete immutable artifact beside its destination and publish
    /// it with one directory rename. The durable journal owns staging on every
    /// error path; readers never scan that reserved staging directory.
    pub(crate) fn publish_artifact_bundle(
        &self,
        relative: &Path,
        text: &str,
        files: &[office_core::domain::artifact::ArtifactFile],
        token: &str,
    ) -> Result<()> {
        self.validate_document_path(relative)?;
        let directory = relative.parent().ok_or_else(|| Error::UnsafePath {
            kind: "artifact",
            value: relative.display().to_string(),
        })?;
        let destination = self.root.join(directory);
        let mut expected = files
            .iter()
            .map(|file| (file.path.as_str(), file.bytes.as_slice()))
            .collect::<Vec<_>>();
        expected.push(("artifact.md", text.as_bytes()));
        match std::fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.is_dir() => {
                // Recovery after publication may only verify, never repair or
                // replace an immutable published directory.
                for (name, bytes) in &expected {
                    self.validate_relative_path(&directory.join(name))?;
                    let path = destination.join(name);
                    let actual = std::fs::read(&path)
                        .map_err(|e| Error::io("verify published artifact", &path, e))?;
                    if actual != *bytes {
                        return Err(Error::Corrupt {
                            path,
                            reason: "published artifact differs from the decided bundle".into(),
                        });
                    }
                }
                return Ok(());
            }
            Ok(_) => {
                return Err(Error::UnsafePath {
                    kind: "occupied artifact",
                    value: directory.display().to_string(),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io("inspect artifact destination", &destination, e)),
        }
        let parent = destination.parent().ok_or_else(|| Error::UnsafePath {
            kind: "artifact",
            value: directory.display().to_string(),
        })?;
        Self::create_private_dirs(parent)?;
        let staging = parent.join(format!(".office-pending-artifact-{token}"));
        refuse_symlink(&staging)?;
        Self::create_private_dirs(&staging)?;
        for (name, bytes) in &expected {
            let path = staging.join(name);
            // Names were validated by the domain. Validate every existing
            // staging component too: a stale journal never grants a symlink.
            let mut physical = staging.clone();
            for component in Path::new(name).components() {
                let std::path::Component::Normal(component) = component else {
                    return Err(Error::UnsafePath {
                        kind: "artifact file",
                        value: (*name).into(),
                    });
                };
                physical.push(component);
                refuse_symlink(&physical)?;
            }
            if let Some(parent) = path.parent() {
                Self::create_private_dirs(parent)?;
            }
            match write_atomic_new(&path, bytes) {
                Ok(()) => {}
                Err(office_common::Error::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    let actual = std::fs::read(&path)
                        .map_err(|e| Error::io("verify staged artifact", &path, e))?;
                    if actual != *bytes {
                        return Err(Error::Corrupt {
                            path,
                            reason: "staged artifact differs from the decided bundle".into(),
                        });
                    }
                }
                Err(error) => return Err(error.into()),
            }
            let mut ancestor = path.parent();
            while let Some(dir) = ancestor {
                std::fs::File::open(dir)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| Error::io("sync staged artifact", dir, e))?;
                if dir == parent {
                    break;
                }
                ancestor = dir.parent();
            }
        }
        // Only plugin writers share this root, serialized by the writer lock.
        // Refuse a newly occupied destination rather than replacing it.
        match std::fs::symlink_metadata(&destination) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(Error::UnsafePath {
                    kind: "occupied artifact",
                    value: directory.display().to_string(),
                });
            }
            Err(e) => return Err(Error::io("inspect artifact destination", &destination, e)),
        }
        std::fs::rename(&staging, &destination)
            .map_err(|e| Error::io("publish artifact bundle", &destination, e))?;
        let mut ancestor = Some(parent);
        while let Some(dir) = ancestor {
            std::fs::File::open(dir)
                .and_then(|file| file.sync_all())
                .map_err(|e| Error::io("sync artifact directory", dir, e))?;
            if dir == self.root {
                break;
            }
            ancestor = dir.parent();
        }
        Ok(())
    }

    /// Names already occupying a capture month, including empty directories,
    /// regular files and dangling symlinks. Inspection failures are not vacancy.
    pub(crate) fn entry_names(&self, relative: &Path) -> Result<Vec<String>> {
        self.validate_relative_path(relative)?;
        let path = self.root.join(relative);
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io("list capture directory", path, e)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::io("list capture directory", &path, e))?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    /// Read and parse one document by its Library-relative path.
    /// A symlink leaf is refused so a scan cannot read through a link.
    pub(crate) fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        let path = self.root.join(relative);
        refuse_symlink(&path)?;
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io("read", &path, e))?;
        let mut record = DocumentRecord::from_markdown(relative, &text);
        if relative.starts_with("sources")
            && relative.file_name().is_some_and(|name| name == "source.md")
        {
            let parent = relative.parent().ok_or_else(|| Error::UnsafePath {
                kind: "source",
                value: relative.display().to_string(),
            })?;
            for name in ["frames.md", "chapters.md", "summary.md"] {
                let companion = parent.join(name);
                self.validate_relative_path(&companion)?;
                match std::fs::read_to_string(self.root.join(&companion)) {
                    Ok(text) => {
                        record.body.push_str("\n\n");
                        record
                            .body
                            .push_str(&office_core::domain::source::source_body(&text));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(Error::io(
                            "read source companion",
                            self.root.join(&companion),
                            error,
                        ));
                    }
                }
            }
            if let Some(text) = self.optional_bootstrap_json(&parent.join("digest.json"))? {
                let digest: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
                    office_core::domain::source::video::refusal(
                        "invalid_bootstrap_data",
                        "digest.json is malformed",
                    )
                })?;
                office_core::domain::bootstrap::digest(&digest)?;
                record.body.push_str("\n\n");
                record.body.push_str(&text);
            }
        }
        Ok(record)
    }

    /// Read a regular shared file only; never open a FIFO or device.
    pub(crate) fn read_shared_bytes(&self, relative: &Path) -> Result<Vec<u8>> {
        self.validate_relative_path(relative)?;
        let path = self.root.join(relative);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| Error::io("inspect shared file", &path, e))?;
        if !metadata.is_file() {
            return Err(Error::UnsafePath {
                kind: "regular shared file",
                value: relative.display().to_string(),
            });
        }
        self.read_bytes(relative)
    }

    pub(crate) fn read_shared_text(&self, relative: &Path) -> Result<String> {
        String::from_utf8(self.read_shared_bytes(relative)?).map_err(|e| {
            Error::io(
                "read shared text",
                self.root.join(relative),
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })
    }

    /// Write one document atomically at a Library-relative path. Every
    /// component of the path is validated first, so a caller-supplied id
    /// can never write outside the Library.
    pub(crate) fn write_document(&self, relative: &Path, text: &str) -> Result<()> {
        self.validate_document_path(relative)?;
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            Self::create_private_dirs(parent)?;
        }
        Ok(write_atomic(&path, text.as_bytes())?)
    }

    pub(crate) fn validate_proposal_path(&self, relative: &Path) -> Result<()> {
        office_core::domain::proposal::validate_record_path(relative)?;
        self.validate_relative_path(relative)
    }

    pub(crate) fn write_proposal(&self, relative: &Path, text: &str) -> Result<()> {
        self.validate_proposal_path(relative)?;
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            Self::create_private_dirs(parent)?;
        }
        Ok(write_atomic(&path, text.as_bytes())?)
    }

    /// Validate every physical path component, including existing ancestors.
    /// A symlink anywhere below the granted root is refused, including a dangling
    /// one. The plugin's filesystem grant is the concurrent replacement boundary.
    pub(crate) fn validate_relative_path(&self, relative: &Path) -> Result<()> {
        if relative.as_os_str().is_empty() {
            return Err(Error::UnsafePath {
                kind: "path",
                value: relative.display().to_string(),
            });
        }
        let mut physical = self.root.clone();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(Error::UnsafePath {
                    kind: "path",
                    value: relative.display().to_string(),
                });
            };
            let name = name.to_str().ok_or_else(|| Error::UnsafePath {
                kind: "path",
                value: relative.display().to_string(),
            })?;
            validate_path_stem(name, "path")?;
            physical.push(name);
            match std::fs::symlink_metadata(&physical) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(Error::UnsafePath {
                        kind: "symlink",
                        value: relative.display().to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io("inspect Library path", &physical, e)),
            }
        }
        Ok(())
    }

    /// Safe Markdown that [`Self::scan_documents`] would index, and not
    /// `original.*`. The repository uses this for document reads, history
    /// and writes.
    pub(crate) fn validate_document_path(&self, relative: &Path) -> Result<()> {
        self.validate_relative_path(relative)?;
        if relative.extension().is_none_or(|ext| ext != "md")
            || relative
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("original."))
        {
            return Err(Error::UnsafePath {
                kind: "document (Markdown, excluding original.*)",
                value: relative.display().to_string(),
            });
        }
        if !is_indexed_document(relative) {
            return Err(Error::UnsafePath {
                kind: INDEXED_DOCUMENT_KIND,
                value: relative.display().to_string(),
            });
        }
        Ok(())
    }

    /// Scan every indexable document in the Library, in path order.
    /// Symlinks are not followed. A missing layer is empty. Any other
    /// failure while inspecting or listing a directory is returned, so a
    /// caller cannot rebuild the index from a partial walk.
    pub(crate) fn scan_documents(&self) -> Result<Vec<DocumentRecord>> {
        let mut files: Vec<PathBuf> = Vec::new();
        for &(layer, stems) in INDEXED_DOCUMENT_LAYERS {
            collect_optional_layer(&self.root.join(layer), &mut files, stems)?;
        }
        files.sort();

        let mut documents = Vec::new();
        for path in files {
            let Ok(relative) = path.strip_prefix(&self.root) else {
                continue;
            };
            documents.push(self.read_document(relative)?);
        }
        Ok(documents)
    }

    /// Scan a clone without following links or admitting out-of-layout files.
    /// The clone root itself is checked by the caller against plugin state.
    pub(crate) fn shared_documents(&self) -> Result<Vec<(DocumentRecord, String)>> {
        let mut paths = Vec::new();
        for layer in ["knowledge", "members"] {
            if let Ok(metadata) = std::fs::symlink_metadata(self.root.join(layer))
                && !metadata.is_dir()
            {
                continue;
            }
            collect_optional_layer(&self.root.join(layer), &mut paths, &[])?;
        }
        paths.sort();
        let mut documents = Vec::new();
        for path in paths {
            let Ok(relative) = path.strip_prefix(&self.root) else {
                continue;
            };
            // Invalid names/symlinks are skipped before reading any bytes.
            match self.validate_relative_path(relative) {
                Ok(()) => {}
                Err(Error::UnsafePath { .. }) => continue,
                Err(error) => return Err(error),
            }
            let markdown = match self.read_shared_text(relative) {
                Ok(text) => text,
                Err(Error::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::InvalidData =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Ok(mut record) = office_core::domain::sharing::shared_document(
                &relative.to_string_lossy(),
                &markdown,
            ) {
                if record.kind == office_core::domain::knowledge::DocumentKind::Source {
                    self.shared_source_body(relative, &mut record)?;
                }
                documents.push((record, markdown));
            }
        }
        Ok(documents)
    }

    /// Index only valid regular text sidecars beside shared evidence. Invalid,
    /// linked or missing companions contribute nothing, never their target bytes.
    pub(crate) fn shared_source_body(
        &self,
        relative: &Path,
        record: &mut DocumentRecord,
    ) -> Result<()> {
        use office_core::domain::{
            bootstrap,
            source::{access, source_body},
        };
        for name in ["frames.md", "chapters.md", "summary.md", "digest.json"] {
            let text = match self.read_shared_text(&relative.with_file_name(name)) {
                Ok(text) => text,
                Err(Error::UnsafePath { .. }) => continue,
                Err(Error::Io { source, .. })
                    if matches!(
                        source.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            let valid = match name {
                "frames.md" => access::frames(&text).is_ok(),
                "chapters.md" => access::chapters(&text).is_ok(),
                "digest.json" => serde_json::from_str::<serde_json::Value>(&text)
                    .is_ok_and(|value| bootstrap::digest(&value).is_ok()),
                "summary.md" => true,
                _ => false,
            };
            if valid {
                record.body.push_str("\n\n");
                record.body.push_str(&source_body(&text));
            }
        }
        Ok(())
    }

    /// True when every bundled template path already exists. An existing
    /// symlink or other entry counts as present so init will not overwrite it.
    pub(crate) fn bundled_templates_present(&self) -> Result<bool> {
        for bundled in BUNDLED {
            if !self.template_entry_exists(bundled.filename)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Copy each bundled template that is missing. Never overwrites an
    /// existing path. Returns the Library-relative paths this call created,
    /// including `.office/templates` when that directory was created.
    pub(crate) fn install_missing_templates(&self) -> Result<Vec<String>> {
        let dir = self.root.join(".office/templates");
        let mut created = Vec::new();
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::UnsafePath {
                    kind: "symlink",
                    value: ".office/templates".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::create_private_dirs(&dir)?;
                created.push(".office/templates".to_string());
            }
            Err(error) => return Err(Error::io("inspect template directory", &dir, error)),
        }
        for bundled in BUNDLED {
            if self.template_entry_exists(bundled.filename)? {
                continue;
            }
            let path = dir.join(bundled.filename);
            write_atomic(&path, bundled.markdown.as_bytes())?;
            let filename = bundled.filename;
            created.push(format!(".office/templates/{filename}"));
        }
        Ok(created)
    }

    /// Template Markdown directly under `.office/templates/`, in path order.
    /// A missing directory is an empty list. A symlink or unreadable file is
    /// returned with `markdown: None` so status can name it and continue.
    pub(crate) fn read_templates(&self) -> Result<Vec<TemplateOnDisk>> {
        let dir = self.root.join(".office/templates");
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Ok(vec![TemplateOnDisk {
                    relative: ".office/templates".to_string(),
                    markdown: None,
                }]);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("inspect template directory", &dir, error)),
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("list templates", &dir, error)),
        };
        let mut templates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| Error::io("list templates", &dir, error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.ends_with(".md") {
                continue;
            }
            let path = entry.path();
            let relative = format!(".office/templates/{name}");
            let markdown = match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => None,
                Ok(_) => std::fs::read_to_string(&path).ok(),
                Err(_) => None,
            };
            templates.push(TemplateOnDisk { relative, markdown });
        }
        templates.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(templates)
    }

    /// Artifact frontmatter files only; companion Markdown is never indexed.
    pub(crate) fn read_artifact_markdown(&self) -> Result<Vec<(String, String)>> {
        let mut paths = Vec::new();
        collect_optional_layer(&self.root.join("artifacts"), &mut paths, &["artifact"])?;
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&self.root)
                    .map_err(|_| Error::UnsafePath {
                        kind: "artifact",
                        value: path.display().to_string(),
                    })?;
                let text = self.read_text(relative)?;
                Ok((relative.to_string_lossy().replace('\\', "/"), text))
            })
            .collect()
    }

    /// Knowledge Markdown, Library-relative path then full text, in path order.
    /// Templates are not included. Symlinks are not followed; a symlink leaf
    /// that still reaches this read is refused. A missing `knowledge`
    /// directory is an empty list. Any other enumeration failure is an error.
    pub(crate) fn read_knowledge_markdown(&self) -> Result<Vec<(String, String)>> {
        self.read_layer_markdown("knowledge", &[])
    }

    /// Every current source's complete Markdown; no search cap or excerpts.
    pub(crate) fn read_source_markdown(&self) -> Result<Vec<(String, String)>> {
        self.read_layer_markdown("sources", &["source"])
    }

    fn read_layer_markdown(&self, layer: &str, stems: &[&str]) -> Result<Vec<(String, String)>> {
        let mut files = Vec::new();
        collect_optional_layer(&self.root.join(layer), &mut files, stems)?;
        files.sort();
        let mut pages = Vec::new();
        for path in files {
            refuse_symlink(&path)?;
            let relative = path.strip_prefix(&self.root).map_err(|_| {
                Error::io(
                    "relativize Library document",
                    &path,
                    std::io::Error::other("path escaped the Library"),
                )
            })?;
            let text = std::fs::read_to_string(&path)
                .map_err(|error| Error::io("read Library document", &path, error))?;
            pages.push((relative.to_string_lossy().replace('\\', "/"), text));
        }
        Ok(pages)
    }

    fn template_entry_exists(&self, filename: &str) -> Result<bool> {
        let path = self.root.join(".office/templates").join(filename);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::io("inspect template", &path, error)),
        }
    }
}

/// Collect `.md` files under one indexed layer.
///
/// A missing directory and a symlink contribute nothing: layers are optional,
/// and a link is not followed (including a link back to an ancestor). Any
/// other inspection or listing failure is returned. `reindex` rebuilds only
/// after every layer walks successfully, so an unreadable directory must not
/// look like an empty one.
fn collect_optional_layer(dir: &Path, out: &mut Vec<PathBuf>, stems: &[&str]) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => collect_markdown(dir, out, stems),
        Err(error) => Err(Error::io("inspect Library directory", dir, error)),
    }
}

/// Collect `.md` files under `dir`; when `stems` is non-empty, only files
/// whose stem is in it (a source directory holds `source.md` beside its
/// `original.*`, and `v2.md` corrections share the source's index row
/// until versions are indexed separately).
///
/// Symlinks are not followed. A symlinked directory is not opened, so a link
/// outside the Library contributes nothing and a link back to an ancestor
/// cannot recurse. Symlinked files are skipped the same way. `Path::is_dir`
/// is not used: it follows links. A directory that cannot be inspected or
/// listed, and a directory entry that cannot be read, fail the walk. Only
/// the layer root treats "not found" as empty; a nested path that disappears
/// after it was listed is an incomplete scan.
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>, stems: &[&str]) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Ok(()),
        Ok(_) => {}
        Err(error) => return Err(Error::io("inspect Library directory", dir, error)),
    }
    let entries =
        std::fs::read_dir(dir).map_err(|error| Error::io("list Library directory", dir, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("list Library directory", dir, error))?;
        let path = entry.path();
        // `DirEntry::file_type` does not follow symlinks.
        let file_type = entry
            .file_type()
            .map_err(|error| Error::io("inspect Library entry", &path, error))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".office-pending-artifact-"))
            {
                continue;
            }
            collect_markdown(&path, out, stems)?;
        } else if file_type.is_file() && markdown_selected(&path, stems) {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether `relative` is a file `scan_documents` would collect.
fn is_indexed_document(relative: &Path) -> bool {
    let mut components = relative.components();
    let Some(std::path::Component::Normal(layer)) = components.next() else {
        return false;
    };
    let Some(layer) = layer.to_str() else {
        return false;
    };
    let Some(&(_, stems)) = INDEXED_DOCUMENT_LAYERS
        .iter()
        .find(|(name, _)| *name == layer)
    else {
        return false;
    };
    let mut inside_layer = false;
    for component in components {
        let std::path::Component::Normal(_) = component else {
            return false;
        };
        inside_layer = true;
    }
    inside_layer && markdown_selected(relative, stems)
}

fn markdown_selected(path: &Path, stems: &[&str]) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
        && (stems.is_empty()
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stems.contains(&stem)))
}

fn refuse_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::UnsafePath {
            kind: "symlink",
            value: path.display().to_string(),
        }),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io("inspect Library path", path, e)),
    }
}
