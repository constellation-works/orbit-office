//! Atomic, owner-only file writes: write a temporary file in the target
//! directory, fsync it, publish it, and fsync the directory. Publication
//! either replaces the target by rename or creates it without replacement
//! by hard link (original uploads must never clobber an existing file).
//! A crash leaves either the old file or the new one, never a half-written
//! file (the `fsio` discipline of the template's store, kept here).

use std::io::Write;
use std::path::{Path, PathBuf};
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

/// The temp path for `seq` in `dir`: process id, sequence and target stem.
///
/// Production mints each candidate through here; crate tests plant entries
/// at the names this produces for the next few sequence numbers.
pub(crate) fn temp_path(dir: &Path, stem: &str, seq: u64) -> PathBuf {
    dir.join(format!(
        ".office-store-{}-{}-{stem}.tmp",
        std::process::id(),
        seq
    ))
}

/// Write `bytes` to `path` atomically, with owner-only permissions.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_counter(path, bytes, &TMP_COUNTER)
}

/// Like [`write_atomic`], minting temp names from `counter` instead of the
/// process-wide counter. Production passes [`TMP_COUNTER`]; crate tests pass
/// their own counter so the next sequence numbers are known in advance.
pub(crate) fn write_atomic_with_counter(
    path: &Path,
    bytes: &[u8],
    counter: &AtomicU64,
) -> Result<()> {
    write_atomic_with(path, bytes, Publication::Replace, counter)
}

/// Atomically create an owner-only file, refusing any existing target.
/// A fully written, synced temp is hard-linked into place, so a crash
/// cannot leave a partial target and publication never replaces an entry.
/// Filesystems without hard-link support return an error without a target.
pub fn write_atomic_new(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with(path, bytes, Publication::Create, &TMP_COUNTER)
}

enum Publication {
    Replace,
    Create,
}

/// Owns only an exclusively created temp, including on error or unwinding.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    publication: Publication,
    counter: &AtomicU64,
) -> Result<()> {
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
        let seq = counter.fetch_add(1, Ordering::Relaxed);
        let tmp = temp_path(dir, &stem, seq);
        let mut file = match open_exclusive(&tmp) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io("write", path, e)),
        };
        let _temp = TempFile(tmp.clone());
        // Creation used mode 0600, which the umask can only narrow, so the
        // file was never more permissive than owner-only. Tighten to exactly
        // 0600 in case the umask stripped an owner bit.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::io("write", path, e))?;
        }
        file.write_all(bytes)
            .map_err(|e| Error::io("write", path, e))?;
        file.sync_all().map_err(|e| Error::io("write", path, e))?;
        drop(file);
        match publication {
            Publication::Replace => std::fs::rename(&tmp, path),
            Publication::Create => std::fs::hard_link(&tmp, path),
        }
        .map_err(|e| Error::io("write", path, e))?;
        drop(_temp);
        // Fsync publication and temp removal before the decision can be cleared.
        std::fs::File::open(dir)
            .and_then(|file| file.sync_all())
            .map_err(|e| Error::io("sync directory", dir, e))?;
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
