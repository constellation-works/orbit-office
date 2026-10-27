//! Reading and writing Library documents on disk, and scanning the whole
//! Library for a reindex.
//!
//! The pure frontmatter reader lives with `DocumentRecord` in office-core;
//! files remain the source of truth for fields the index does not consume.

use std::path::{Path, PathBuf};

use crate::contracts::DocumentRecord;
use crate::error::{Error, Result};
use office_common::fs::atomic::write_atomic;
use office_common::fs::path_safety::validate_path_stem;

/// Directory layout created under a Library root.
pub(crate) const LAYOUT_DIRS: &[&str] = &[
    "sources",
    "knowledge/domain",
    "knowledge/runbooks",
    "artifacts",
    "proposals",
    ".office",
];

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

    /// Read internal settings without following a symlink out of the Library.
    pub(crate) fn settings_text(&self) -> Result<Option<String>> {
        let parent = self.root.join(".office");
        refuse_symlink(&parent)?;
        let path = parent.join("config.yaml");
        refuse_symlink(&path)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io("read settings", path, e)),
        }
    }

    /// Read and parse one document by its Library-relative path.
    pub(crate) fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        let path = self.root.join(relative);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io("read", &path, e))?;
        Ok(DocumentRecord::from_markdown(relative, &text))
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
        Ok(())
    }

    /// Scan every indexable document in the Library, in path order.
    pub(crate) fn scan_documents(&self) -> Result<Vec<DocumentRecord>> {
        let mut files: Vec<PathBuf> = Vec::new();
        collect_markdown(&self.root.join("sources"), &mut files, &["source"]);
        collect_markdown(&self.root.join("knowledge"), &mut files, &[]);
        collect_markdown(&self.root.join("artifacts"), &mut files, &["artifact"]);
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
}

/// Collect `.md` files under `dir`; when `stems` is non-empty, only files
/// whose stem is in it (a source directory holds `source.md` beside its
/// `original.*`, and `v2.md` corrections share the source's index row
/// until versions are indexed separately).
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>, stems: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out, stems);
        } else if path.extension().is_some_and(|ext| ext == "md")
            && (stems.is_empty()
                || path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stems.contains(&stem)))
        {
            out.push(path);
        }
    }
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
