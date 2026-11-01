//! The scribe's parsed message files (specs/scribe.md, "Pipeline"):
//! `<library>/.office/state/scribe/<session-id>.jsonl` and its
//! `<session-id>.meta.json` session facts, owner-only in a private
//! directory and rewritten on every parse. The Library's `.gitignore`
//! ignores `.office/state/`, and the directory is inside the plugin's
//! `{{workspace}}/library` write grant. Also the host facts an SCR file
//! records: the host name and the local time zone.

use std::path::{Component, Path, PathBuf};

use office_common::fs::atomic::write_atomic;
use office_core::domain::scribe_control::validate_session_id;
use office_core::domain::source::conversation::scribe::{PARSE_DIR, parse_path};
use office_core::ports::ScribeParse;

use crate::repository::normalize_path;
use crate::{Error, Result};

/// Parsed scribe files under one Library root.
pub struct ScribeParseStore {
    library: PathBuf,
    /// The Library root relative to the workspace, `/`-separated.
    library_dir: String,
}

fn refuse_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::UnsafePath {
            kind: "symlink",
            value: path.display().to_string(),
        }),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io("inspect scribe path", path, e)),
    }
}

impl ScribeParseStore {
    /// The store for the Library at `library_root`, which must lie under
    /// `workspace_root` (callers apply the Library grant first). Paths are
    /// compared lexically; construction does no I/O.
    pub fn new(workspace_root: &Path, library_root: &Path) -> Result<Self> {
        let workspace = normalize_path(workspace_root);
        let library = normalize_path(library_root);
        let relative = library
            .strip_prefix(&workspace)
            .map_err(|_| Error::LibraryDirOutside {
                requested: library.clone(),
                allowed: workspace.clone(),
            })?;
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(name) => match name.to_str() {
                    Some(name) => parts.push(name),
                    None => {
                        return Err(Error::UnsafePath {
                            kind: "Library directory",
                            value: library.display().to_string(),
                        });
                    }
                },
                Component::CurDir
                | Component::ParentDir
                | Component::RootDir
                | Component::Prefix(_) => {
                    return Err(Error::LibraryDirOutside {
                        requested: library.clone(),
                        allowed: workspace.clone(),
                    });
                }
            }
        }
        let library_dir = parts.join("/");
        Ok(Self {
            library,
            library_dir,
        })
    }

    fn dir(&self) -> PathBuf {
        self.library.join(PARSE_DIR)
    }

    /// The workspace-relative path of a session's message file.
    pub fn relative_path(&self, session_id: &str) -> office_core::Result<String> {
        validate_session_id(session_id)?;
        Ok(parse_path(&self.library_dir, session_id))
    }

    fn paths(&self, session_id: &str) -> office_core::Result<(PathBuf, PathBuf)> {
        validate_session_id(session_id)?;
        let dir = self.dir();
        Ok((
            dir.join(format!("{session_id}.jsonl")),
            dir.join(format!("{session_id}.meta.json")),
        ))
    }

    /// Refuse a symlink at any directory between the Library root and
    /// the parse directory, so a write never leaves the Library.
    fn refuse_linked_dirs(&self) -> Result<()> {
        let mut path = self.library.clone();
        refuse_symlink(&path)?;
        for part in Path::new(PARSE_DIR).components() {
            path.push(part);
            refuse_symlink(&path)?;
        }
        Ok(())
    }

    /// Replace a session's message and fact files; returns the message
    /// file's workspace-relative path.
    pub fn write(
        &self,
        session_id: &str,
        messages: &str,
        meta: &str,
    ) -> office_core::Result<String> {
        let (jsonl, facts) = self.paths(session_id)?;
        let dir = self.dir();
        self.refuse_linked_dirs()?;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder
            .create(&dir)
            .map_err(|e| Error::io("create scribe parse directory", &dir, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| Error::io("restrict scribe parse directory", &dir, e))?;
        }
        for (path, text) in [(&jsonl, messages), (&facts, meta)] {
            refuse_symlink(path)?;
            write_atomic(path, text.as_bytes()).map_err(Error::from)?;
        }
        self.relative_path(session_id)
    }

    /// A session's message and fact files, if both exist.
    pub fn read(&self, session_id: &str) -> office_core::Result<Option<ScribeParse>> {
        let (jsonl, facts) = self.paths(session_id)?;
        self.refuse_linked_dirs()?;
        let mut texts = Vec::new();
        for path in [&jsonl, &facts] {
            refuse_symlink(path)?;
            match std::fs::read_to_string(path) {
                Ok(text) => texts.push(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(Error::io("read scribe parse file", path, e).into()),
            }
        }
        let meta = texts.pop().unwrap_or_default();
        let messages = texts.pop().unwrap_or_default();
        Ok(Some(ScribeParse { messages, meta }))
    }
}

/// The host's name: `/etc/hostname`, else `uname -n`; `None` when unknown.
#[must_use]
pub fn host_name() -> Option<String> {
    let clean = |text: &str| {
        let name = text.trim();
        (!name.is_empty() && name.len() <= 255 && !name.chars().any(char::is_control))
            .then(|| name.to_owned())
    };
    if let Ok(text) = std::fs::read_to_string("/etc/hostname")
        && let Some(name) = clean(&text)
    {
        return Some(name);
    }
    let mut command = std::process::Command::new("uname");
    command.arg("-n");
    let output = office_common::process::run_captured(&mut command, Path::new("/")).ok()?;
    if !output.status.success() {
        return None;
    }
    clean(&String::from_utf8_lossy(&output.stdout))
}

/// The host's IANA time zone: `TZ`, else the `/etc/localtime` link's
/// `zoneinfo/` suffix; `None` (UTC) when neither names one.
#[must_use]
pub fn time_zone() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if !tz.is_empty() {
            return Some(tz.to_owned());
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_string_lossy();
    target
        .split_once("zoneinfo/")
        .map(|(_, zone)| zone.to_owned())
        .filter(|zone| !zone.is_empty())
}
