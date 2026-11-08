//! Markdown layer collection and symlink refusal.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

use super::layout::markdown_selected;

/// Collect `.md` files under one indexed layer.
///
/// A missing directory and a symlink contribute nothing: layers are optional,
/// and a link is not followed (including a link back to an ancestor). Any
/// other inspection or listing failure is returned. `reindex` rebuilds only
/// after every layer walks successfully, so an unreadable directory must not
/// look like an empty one.
pub(super) fn collect_optional_layer(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    stems: &[&str],
) -> Result<()> {
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

pub(super) fn refuse_symlink(path: &Path) -> Result<()> {
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
