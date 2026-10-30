//! Grant and path confinement: deciding a Library root inside a workspace.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// A Library root that resolved inside `{{workspace}}/library`.
pub(super) struct GrantedRoot {
    pub(super) candidate: PathBuf,
    pub(super) allowed: PathBuf,
}

/// Decide containment on the physical path before any create (STD-05 R6).
///
/// Lexical `..` and absolute paths are normalized first so a join cannot
/// spell a different root. A missing suffix is appended to its canonical
/// ancestor; the spelled path alone is never the decision.
///
/// The grant is the workspace's own `library` directory: the workspace is
/// resolved, `library` is not. A `library` link, even to a directory inside
/// the workspace, is refused, so its target never becomes the allowed root.
pub(super) fn resolve_within_grant(candidate: &Path, workspace_root: &Path) -> Result<GrantedRoot> {
    let allowed = normalize_path(&workspace_root.join("library"));
    let candidate = normalize_path(candidate);
    if candidate != allowed && !candidate.starts_with(&allowed) {
        return Err(Error::LibraryDirOutside {
            requested: candidate,
            allowed,
        });
    }
    if std::fs::symlink_metadata(&candidate).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(Error::UnsafePath {
            kind: "symlink",
            value: candidate.display().to_string(),
        });
    }
    if std::fs::symlink_metadata(&allowed).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::LibraryDirOutside {
            requested: candidate,
            allowed,
        });
    }
    // Resolve the grant and destination before creating the lock, index or
    // Library. A missing suffix is appended to its canonical ancestor.
    let allowed = resolve_existing_ancestor(&normalize_path(workspace_root))?.join("library");
    let candidate = resolve_existing_ancestor(&candidate)?;
    if !candidate.starts_with(&allowed) {
        return Err(Error::LibraryDirOutside {
            requested: candidate,
            allowed,
        });
    }
    Ok(GrantedRoot { candidate, allowed })
}

/// Lexically resolve `.` and `..` without requiring the path to exist.
pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            std::path::Component::RootDir => out.push(std::path::Component::RootDir.as_os_str()),
            std::path::Component::Normal(name) => out.push(name),
        }
    }
    out
}

/// Resolve an existing ancestor without treating dangling links as missing.
pub(super) fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = ancestor
                    .canonicalize()
                    .map_err(|e| Error::io("resolve Library path", ancestor, e))?;
                for name in missing.into_iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| Error::io("resolve Library path", ancestor, e))?;
                missing.push(name);
                ancestor = ancestor.parent().ok_or_else(|| {
                    Error::io(
                        "resolve Library path",
                        ancestor,
                        std::io::Error::other("no existing ancestor"),
                    )
                })?;
            }
            Err(e) => return Err(Error::io("inspect Library path", ancestor, e)),
        }
    }
}
