//! Atomic, owner-only file writes: write a temporary file in the target
//! directory, fsync it, rename over the target, and fsync the directory.
//! A crash leaves either the old file or the new one, never a half-written
//! file (the `fsio` discipline of the template's store, kept here).

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, Result};

/// Process-wide counter that makes each temp name unique within this process.
///
/// Combined with the process id, every `write_atomic` call (and every retry)
/// gets its own temp path, so concurrent writers to the same target never
/// share one.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// How many distinct temp names to try before giving up.
///
/// Collisions are only expected from a pre-existing entry (possibly planted)
/// at a candidate name; each attempt mints a fresh name, so this bound is
/// generous while keeping a wedged directory from spinning forever.
const MAX_TMP_ATTEMPTS: u32 = 100;

/// Write `bytes` to `path` atomically, with owner-only permissions.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::Io {
        action: "resolve parent directory of",
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io("create directory", dir, e))?;

    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_owned();

    for _ in 0..MAX_TMP_ATTEMPTS {
        let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(
            ".office-store-{}-{}-{stem}.tmp",
            std::process::id(),
            seq
        ));
        let mut file = match open_exclusive(&tmp) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io("write", path, e)),
        };
        // Creation used mode 0600, which the umask can only narrow, so the
        // file was never more permissive than owner-only. Tighten to exactly
        // 0600 in case the umask stripped an owner bit.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                let _ = std::fs::remove_file(&tmp);
                return Err(Error::io("write", path, e));
            }
        }
        // Each fallible step below maps its failure to `Error::io` and
        // removes only this attempt's exclusively created temp; a
        // pre-existing entry at another name is never touched.
        if let Err(source) = file.write_all(bytes) {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::io("write", path, source));
        }
        if let Err(source) = file.sync_all() {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::io("write", path, source));
        }
        drop(file);
        if let Err(source) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::io("write", path, source));
        }
        // Fsync the directory so the rename itself is durable.
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
        return Ok(());
    }
    Err(Error::io(
        "write",
        path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not pick an unused temporary file name",
        ),
    ))
}

/// Open `tmp` exclusively: fail with [`std::io::ErrorKind::AlreadyExists`]
/// when anything — a file, a directory, or a symlink — already exists there,
/// instead of truncating or following it.
fn open_exclusive(tmp: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(tmp)
}
