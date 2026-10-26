//! Atomic, owner-only file writes: write a temporary file in the target
//! directory, fsync it, rename over the target, and fsync the directory.
//! A crash leaves either the old file or the new one, never a half-written
//! file (the `fsio` discipline of the template's store, kept here).

use std::io::Write;
use std::path::Path;

use crate::{Error, Result};

/// Write `bytes` to `path` atomically, with owner-only permissions.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::Io {
        action: "resolve parent directory of",
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io("create directory", dir, e))?;

    let tmp = dir.join(format!(
        ".office-store-{}-{}.tmp",
        std::process::id(),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    let write = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        // Fsync the directory so the rename itself is durable.
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
        Ok(())
    })();
    if let Err(source) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::io("write", path, source));
    }
    Ok(())
}
