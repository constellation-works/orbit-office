//! Reading and writing Library documents on disk, and scanning the indexed
//! layers for a reindex.
//!
//! The pure frontmatter reader lives with `DocumentRecord` in office-core;
//! files remain the source of truth for fields the index does not consume.

use std::path::{Path, PathBuf};

use crate::contracts::DocumentRecord;
use crate::error::{Error, Result};
use office_common::fs::atomic::write_atomic;
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
    pub(crate) fn scan_documents(&self) -> Result<Vec<DocumentRecord>> {
        let mut files: Vec<PathBuf> = Vec::new();
        for &(layer, stems) in INDEXED_DOCUMENT_LAYERS {
            collect_markdown(&self.root.join(layer), &mut files, stems);
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

    /// Knowledge Markdown, Library-relative path then full text, in path order.
    /// Templates are not included.
    pub(crate) fn read_knowledge_markdown(&self) -> Result<Vec<(String, String)>> {
        let mut files = Vec::new();
        collect_markdown(&self.root.join("knowledge"), &mut files, &[]);
        files.sort();
        let mut pages = Vec::new();
        for path in files {
            refuse_symlink(&path)?;
            let relative = path.strip_prefix(&self.root).map_err(|_| {
                Error::io(
                    "relativize knowledge page",
                    &path,
                    std::io::Error::other("path escaped the Library"),
                )
            })?;
            let text = std::fs::read_to_string(&path)
                .map_err(|error| Error::io("read knowledge page", &path, error))?;
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
        } else if markdown_selected(&path, stems) {
            out.push(path);
        }
    }
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
