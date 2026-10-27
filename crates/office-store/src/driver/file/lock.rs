//! One kernel advisory writer lock per plugin state, with bounded acquisition.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{Error, Result};
use office_common::fs::atomic::write_atomic;

/// Closing this owner releases the kernel lock, including during unwinding.
/// Lock order: writer lock first, then the short-lived SQLite connection mutex.
pub(crate) struct WriterLock {
    _file: File,
}

impl WriterLock {
    pub(crate) fn acquire(state: &Path) -> Result<Self> {
        let path = state.join("library.writer.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // std opens close-on-exec. Each acquisition opens a fresh descriptor;
        // cloning a descriptor would share the same OS lock on some platforms.
        let file = options
            .open(&path)
            .map_err(|e| Error::io("open writer lock", &path, e))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        let mut text = String::new();
                        let holder = File::open(state.join("library.writer.owner.json"))
                            .and_then(|f| f.take(4096).read_to_string(&mut text))
                            .ok().and_then(|_| serde_json::from_str::<serde_json::Value>(&text).ok())
                            .and_then(|v| Some(format!("pid={}, acquired_at_unix_secs={}, label=Library write/index/commit", v.get("pid")?.as_u64()?, v.get("acquired_at_unix_secs")?.as_u64()?)))
                            .unwrap_or_else(|| "holder unavailable".into());
                        return Err(Error::LockTimeout { path, holder });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::Error(e)) => {
                    return Err(Error::io("acquire writer lock", &path, e));
                }
            }
        }
        let guard = Self { _file: file };
        let acquired = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                Error::io(
                    "read lock acquisition time",
                    &path,
                    std::io::Error::other(e),
                )
            })?
            .as_secs();
        let holder = serde_json::json!({"pid": std::process::id(), "acquired_at_unix_secs": acquired, "label": "Library write/index/commit"});
        write_atomic(
            &state.join("library.writer.owner.json"),
            holder.to_string().as_bytes(),
        )?;
        Ok(guard)
    }
}
