//! Reading and writing Library documents on disk, and scanning the whole
//! Library for a reindex.
//!
//! Frontmatter is parsed with a deliberately small reader for the fields
//! the index needs (`id`, `title`, `summary`, `tags`, `series`,
//! `sensitivity`, `cites`, `related`). It is not a YAML engine: inline
//! lists (`[a, b]`) and block lists (`- item`) are the two shapes the
//! Library conventions use, and anything else is kept as a plain string.

use std::path::{Path, PathBuf};

use crate::contracts::{DocumentKind, DocumentRecord};
use crate::error::{Error, Result};
use office_common::fs::atomic::write_atomic;
use office_common::fs::path_safety::validate_path_stem;

/// Directory layout created under a Library root.
const LAYOUT_DIRS: &[&str] = &[
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
            let path = self.root.join(dir);
            std::fs::create_dir_all(&path).map_err(|e| Error::io("create directory", &path, e))?;
        }
        Ok(())
    }

    /// Read and parse one document by its Library-relative path.
    pub(crate) fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        let path = self.root.join(relative);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io("read", &path, e))?;
        parse_document(&text, relative).map_err(|reason| Error::Corrupt {
            path,
            reason: reason.to_string(),
        })
    }

    /// Write one document atomically at a Library-relative path. Every
    /// component of the path is validated first, so a caller-supplied id
    /// can never write outside the Library.
    pub(crate) fn write_document(&self, relative: &Path, text: &str) -> Result<()> {
        for component in relative.components() {
            let name = component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Error::UnsafePath {
                    kind: "path",
                    value: relative.display().to_string(),
                })?;
            validate_path_stem(name, "path")?;
        }
        Ok(write_atomic(&self.root.join(relative), text.as_bytes())?)
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

/// Parse one document's frontmatter and body into an index record.
fn parse_document(
    text: &str,
    relative: &Path,
) -> std::result::Result<DocumentRecord, &'static str> {
    let (frontmatter, body) = split_frontmatter(text);
    let kind = kind_for_path(relative);

    let mut record = DocumentRecord {
        id: String::new(),
        kind,
        title: String::new(),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".to_string(),
        path: relative.to_string_lossy().replace('\\', "/"),
        body: body.trim().to_string(),
        cites: Vec::new(),
        related: Vec::new(),
    };

    let mut list_key: Option<&str> = None;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            match list_key {
                Some("cites") => record.cites.push(unquote(item.trim())),
                Some("related") => record.related.push(unquote(item.trim())),
                Some("tags") => record.tags.push(unquote(item.trim())),
                _ => {}
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        list_key = None;
        match key {
            "id" => record.id = unquote(value),
            "title" => record.title = unquote(value),
            "summary" => record.summary = unquote(value),
            "tags" => {
                record.tags = inline_list(value);
                list_key = Some("tags");
            }
            "series" if !value.is_empty() && value != "null" => {
                record.series = Some(unquote(value));
            }
            "sensitivity" if !value.is_empty() => record.sensitivity = unquote(value),
            "cites" => {
                record.cites = inline_list(value);
                list_key = Some("cites");
            }
            "related" => {
                record.related = inline_list(value);
                list_key = Some("related");
            }
            _ => {}
        }
    }

    if record.id.is_empty() {
        record.id = relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
    }
    if record.title.is_empty() {
        record.title = record.id.clone();
    }
    Ok(record)
}

/// Library layer from a document's relative path.
fn kind_for_path(relative: &Path) -> DocumentKind {
    let text = relative.to_string_lossy();
    if text.starts_with("knowledge/domain") {
        DocumentKind::Domain
    } else if text.starts_with("knowledge/runbooks") {
        DocumentKind::Runbook
    } else if text.starts_with("artifacts") {
        DocumentKind::Artifact
    } else {
        DocumentKind::Source
    }
}

/// Split leading `---` frontmatter from the body. A document without
/// frontmatter parses as all body.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return ("", text);
    };
    match rest.find("\n---") {
        Some(end) => {
            let body_start = end + "\n---".len();
            let body = rest[body_start..]
                .strip_prefix('\n')
                .unwrap_or(&rest[body_start..]);
            (&rest[..end], body)
        }
        None => ("", text),
    }
}

/// `[a, b]` or a bare scalar, as a list of unquoted items.
fn inline_list(value: &str) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner.split(',').map(|item| unquote(item.trim())).collect()
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
        .to_string()
}
