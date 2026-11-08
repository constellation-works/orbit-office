//! Owner-only filesystem permissions: the one place that sets `0o700`
//! directories and `0o600` files, and creates directories with that mode.
//! Callers in domain crates translate the common [`Error`] into their
//! own error type at their boundary.
//!
//! [`Error`]: crate::Error

use std::path::Path;

use crate::{Error, Result};

/// Best-effort owner-only permissions (`0o700` for a directory, `0o600`
/// for a file): a chmod failure is ignored, and non-Unix hosts are a
/// no-op. Use after creating a path whose creation mode was not controlled
/// (for example a SQLite database file); use [`ensure_owner_only`] where a
/// failure must surface.
pub fn set_owner_only(path: &Path, is_dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, is_dir);
}

/// Owner-only permissions with the failure returned (see
/// [`set_owner_only`] for the modes). A no-op success on non-Unix hosts.
pub fn ensure_owner_only(path: &Path, is_dir: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|source| Error::io("set permissions on", path, source))?;
    }
    #[cfg(not(unix))]
    let _ = (path, is_dir);
    Ok(())
}

/// Create `path` and any missing parents with owner-only permissions.
/// Only directories this call creates get the mode; existing directories
/// keep theirs, matching recursive `DirBuilder` semantics.
pub fn create_private_dirs(path: &Path) -> Result<()> {
    private_dir_builder(true)
        .create(path)
        .map_err(|source| Error::io("create directory", path, source))
}

/// Create only the leaf directory `path` with owner-only permissions,
/// failing when it already exists or its parent is missing. Callers that
/// tolerate an existing leaf match on
/// [`std::io::ErrorKind::AlreadyExists`] in the returned
/// [`Error::Io`] source.
pub fn create_private_dir(path: &Path) -> Result<()> {
    private_dir_builder(false)
        .create(path)
        .map_err(|source| Error::io("create directory", path, source))
}

fn private_dir_builder(recursive: bool) -> std::fs::DirBuilder {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}
