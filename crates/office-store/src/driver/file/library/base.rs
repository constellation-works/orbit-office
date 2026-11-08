//! Library root, layout creation, internal files and path validation.

use std::path::{Path, PathBuf};

use office_common::fs::atomic::write_atomic;
use office_common::fs::path_safety::validate_path_stem;

use crate::error::{Error, Result};

use super::LibraryFs;
use super::collection::refuse_symlink;
use super::layout::{INDEXED_DOCUMENT_KIND, LAYOUT_DIRS, is_indexed_document};

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
}
