//! Scanning and reading indexed layers, ledgers and shared documents.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use office_core::domain::knowledge::DocumentRecord;

use super::LibraryFs;
use super::collection::{collect_optional_layer, refuse_symlink};
use super::layout::INDEXED_DOCUMENT_LAYERS;

impl LibraryFs {
    /// Every SCR file under `sources/conversation/<yyyy>/<mm>/`, in path
    /// order. Symlinks and other names are skipped, never followed.
    pub(crate) fn scribe_markdown(&self) -> Result<Vec<(String, String)>> {
        fn entries(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
            match std::fs::symlink_metadata(dir) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Ok(vec![]),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
                Err(e) => return Err(Error::io("inspect Library directory", dir, e)),
            }
            std::fs::read_dir(dir)
                .map_err(|e| Error::io("list Library directory", dir, e))?
                .map(|entry| entry.map_err(|e| Error::io("list Library directory", dir, e)))
                .collect()
        }
        let mut files = Vec::new();
        let base = self.root.join("sources/conversation");
        for year in entries(&base)? {
            for month in entries(&year.path())? {
                for file in entries(&month.path())? {
                    let path = file.path();
                    let Ok(relative) = path.strip_prefix(&self.root) else {
                        continue;
                    };
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    let is_file = file
                        .file_type()
                        .map_err(|e| Error::io("inspect SCR file", &path, e))?
                        .is_file();
                    if is_file
                        && office_core::domain::source::conversation::scribe::is_scr_path(&relative)
                    {
                        let text = std::fs::read_to_string(&path)
                            .map_err(|e| Error::io("read SCR file", &path, e))?;
                        files.push((relative, text));
                    }
                }
            }
        }
        files.sort();
        Ok(files)
    }

    /// Every proposal page under `proposals/`, in path order. Symlinks are
    /// not followed; files that are not proposal page paths (`.gitkeep`) are
    /// skipped. A page that cannot be read as UTF-8 has no Markdown.
    pub(crate) fn proposal_pages(&self) -> Result<Vec<office_core::ports::LibraryText>> {
        let mut files = Vec::new();
        collect_optional_layer(&self.root.join("proposals"), &mut files, &[])?;
        files.sort();
        let mut pages = Vec::new();
        for path in files {
            let Ok(relative) = path.strip_prefix(&self.root) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            if office_core::domain::proposal::parse_record_path(Path::new(relative)).is_err() {
                continue;
            }
            pages.push(office_core::ports::LibraryText {
                path: relative.replace(std::path::MAIN_SEPARATOR, "/"),
                markdown: std::fs::read_to_string(&path).ok(),
            });
        }
        Ok(pages)
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
        files.extend(self.ledger_files()?);
        files.sort();
        files.dedup();

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

    /// Every decision ledger file (`sources/decisions/<scope>.md`), in path
    /// order. A missing directory is empty; symlinks are never followed.
    fn ledger_files(&self) -> Result<Vec<PathBuf>> {
        let dir = self
            .root
            .join(office_core::domain::source::decisions::LEDGER_DIR);
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if !metadata.is_dir() => return Ok(Vec::new()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(Error::io("inspect decision ledgers", &dir, error)),
        }
        let mut files = Vec::new();
        for entry in
            std::fs::read_dir(&dir).map_err(|e| Error::io("list decision ledgers", &dir, e))?
        {
            let entry = entry.map_err(|e| Error::io("list decision ledgers", &dir, e))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| Error::io("inspect decision ledger", &path, e))?;
            if !file_type.is_file() {
                continue;
            }
            if path
                .strip_prefix(&self.root)
                .is_ok_and(office_core::domain::source::decisions::is_ledger_path)
            {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    /// Every decision ledger's path and Markdown, in path order.
    pub(crate) fn read_precedent_ledgers(&self) -> Result<Vec<office_core::ports::LibraryText>> {
        self.validate_relative_path(Path::new(
            office_core::domain::source::decisions::LEDGER_DIR,
        ))?;
        let dir = self
            .root
            .join(office_core::domain::source::decisions::LEDGER_DIR);
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(Error::Corrupt {
                    path: dir,
                    reason: "decision ledger directory is not a contained directory".into(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("inspect decision ledgers", &dir, error)),
        }
        let mut out = Vec::new();
        for entry in
            std::fs::read_dir(&dir).map_err(|e| Error::io("list decision ledgers", &dir, e))?
        {
            let path = entry
                .map_err(|e| Error::io("list decision ledgers", &dir, e))?
                .path();
            let relative = path.strip_prefix(&self.root).map_err(|_| Error::Corrupt {
                path: path.clone(),
                reason: "ledger escaped the Library".into(),
            })?;
            if office_core::domain::source::decisions::is_ledger_path(relative) {
                // Never open a FIFO, device, directory or symlink as text.
                // Still retain its identity so the provider reports the gap.
                let markdown = std::fs::symlink_metadata(&path)
                    .ok()
                    .filter(|metadata| metadata.is_file())
                    .and_then(|_| self.read_text(relative).ok());
                out.push(office_core::ports::LibraryText {
                    path: relative.to_string_lossy().replace('\\', "/"),
                    markdown,
                });
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Every decision ledger's path and Markdown, in path order.
    pub(crate) fn read_ledger_markdown(&self) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for path in self.ledger_files()? {
            refuse_symlink(&path)?;
            let text = std::fs::read_to_string(&path)
                .map_err(|error| Error::io("read decision ledger", &path, error))?;
            let relative = path.strip_prefix(&self.root).map_err(|_| {
                Error::io(
                    "relativize decision ledger",
                    &path,
                    std::io::Error::other("path escaped the Library"),
                )
            })?;
            out.push((relative.to_string_lossy().replace('\\', "/"), text));
        }
        Ok(out)
    }

    /// Every current source's complete Markdown; no search cap or excerpts.
    pub(crate) fn read_source_markdown(&self) -> Result<Vec<(String, String)>> {
        self.read_layer_markdown("sources", &["source"])
    }

    pub(super) fn read_layer_markdown(
        &self,
        layer: &str,
        stems: &[&str],
    ) -> Result<Vec<(String, String)>> {
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
}
