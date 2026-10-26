//! Path safety for ids that become Library path components.
//!
//! After orbit-store's `fs/path_safety`: an id is accepted only when it is
//! exactly one normal path component — no separators, no traversal, no
//! leading dot — so a caller-supplied id can never escape the Library root.

use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

/// Validate that `stem` is a single safe path component for a `kind` id.
pub fn validate_path_stem(stem: &str, kind: &'static str) -> Result<()> {
    let mut components = Path::new(stem).components();
    let safe = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(part)), None) if part.to_str() == Some(stem)
    ) && !stem.starts_with('.');
    if safe {
        return Ok(());
    }
    Err(Error::UnsafePath {
        kind,
        value: stem.to_string(),
    })
}

/// Normalize a path without touching the filesystem: resolve `.` and `..`
/// lexically, preferring the canonical path when it exists. Kept with the
/// path-safety machinery for the write paths that resolve caller-supplied
/// roots (capture, artifact promotion).
#[allow(dead_code)]
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
