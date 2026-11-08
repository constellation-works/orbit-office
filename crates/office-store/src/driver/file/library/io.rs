//! Single-file reads and writes within the Library.

use std::path::Path;

use office_common::fs::atomic::{write_atomic, write_atomic_new};

use crate::contracts::DocumentRecord;
use crate::error::{Error, Result};

use super::LibraryFs;
use super::collection::refuse_symlink;

impl LibraryFs {
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

    /// A legacy SCR file path (specs/decisions.md, "Legacy SCR files"), checked component by component.
    pub(crate) fn validate_scribe_path(&self, relative: &Path) -> Result<()> {
        office_core::domain::source::conversation::scribe::validate_scr_path(relative)?;
        self.validate_relative_path(relative)
    }

    /// Write one SCR file atomically, owner-only.
    pub(crate) fn write_scribe(&self, relative: &Path, text: &str) -> Result<()> {
        self.validate_scribe_path(relative)?;
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            Self::create_private_dirs(parent)?;
        }
        Ok(write_atomic(&path, text.as_bytes())?)
    }

    /// Remove an SCR file's previous name; an absent file is not an error,
    /// so a replayed rename is idempotent.
    pub(crate) fn remove_scribe(&self, relative: &Path) -> Result<()> {
        self.validate_scribe_path(relative)?;
        let path = self.root.join(relative);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::io("remove renamed SCR file", path, error)),
        }
    }

    /// Remove one proposal page; an already absent page is not an error, so a
    /// replayed move is idempotent.
    pub(crate) fn remove_proposal(&self, relative: &Path) -> Result<()> {
        self.validate_proposal_path(relative)?;
        let path = self.root.join(relative);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::io("remove moved proposal page", path, error)),
        }
    }
}
