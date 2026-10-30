//! Atomic private calendar state; construction and reads never create files.
use crate::driver::file::lock::WriterLock;
use office_common::fs::{atomic::write_atomic, hash::sha256_hex};
use office_core::{
    Result,
    domain::calendar::{CalendarError, CalendarSources, LocalCalendar, LocalChange, Snapshot},
    ports::Calendar,
};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Snapshot adapter rooted only in host-supplied paths.
pub struct CalendarStore {
    state: PathBuf,
    workspace: PathBuf,
}
impl CalendarStore {
    /// Resolve no ambient paths and perform no I/O.
    #[must_use]
    pub fn new(state: PathBuf, workspace: PathBuf) -> Self {
        Self { state, workspace }
    }
    fn dir(&self, create: bool) -> Result<PathBuf> {
        let state = resolved_root(&self.state)?;
        let dir = state.join("calendar");
        if create {
            // Refuse symlink components before writing in plugin state.
            safe_components(&state)?;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&state).map_err(|_| CalendarError::Storage)?;
        }
        safe_components(&dir)?;
        if create {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(CalendarError::Storage.into()),
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                    .map_err(|_| CalendarError::Storage)?;
            }
        }
        Ok(dir)
    }
    fn bytes(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        safe_components(path)?;
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CalendarError::Storage.into()),
        };
        if !file
            .metadata()
            .map_err(|_| CalendarError::Storage)?
            .is_file()
        {
            return Err(CalendarError::Storage.into());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1_048_576 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CalendarError::Storage)?;
        if bytes.len() > 16 * 1_048_576 {
            return Err(CalendarError::TooLarge.into());
        }
        Ok(Some(bytes))
    }
    fn hash(&self, dir: &Path) -> Result<String> {
        Ok(sha256_hex(
            &self.bytes(&dir.join("events.json"))?.unwrap_or_default(),
        ))
    }
}
fn resolved_root(path: &Path) -> Result<PathBuf> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(CalendarError::Storage.into());
    }
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().ok_or(CalendarError::Storage)?);
        ancestor = ancestor.parent().ok_or(CalendarError::Storage)?;
    }
    let mut root = ancestor
        .canonicalize()
        .map_err(|_| CalendarError::Storage)?;
    for part in suffix.into_iter().rev() {
        root.push(part);
    }
    Ok(root)
}
fn safe_components(path: &Path) -> Result<()> {
    let mut at = PathBuf::new();
    for part in path.components() {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return Err(CalendarError::Storage.into()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CalendarError::Storage.into()),
        }
    }
    Ok(())
}
impl CalendarStore {
    fn local_store(&self) -> Result<Self> {
        // Keep authored events inside the workspace, independently of plugin cache resets.
        let workspace = resolved_root(&self.workspace)?;
        safe_components(&workspace.join(".office"))?;
        Ok(Self::new(workspace.join(".office"), workspace))
    }
    fn local_state(&self) -> Result<LocalCalendar> {
        let store = self.local_store()?;
        let dir = store.dir(false)?;
        let Some(bytes) = store.bytes(&dir.join("local.json"))? else {
            return Ok(LocalCalendar::default());
        };
        let state: LocalCalendar =
            serde_json::from_slice(&bytes).map_err(|_| CalendarError::Storage)?;
        if state.schema_version != 1 || state.events.iter().any(|e| !e.local) {
            return Err(CalendarError::Storage.into());
        }
        Ok(state)
    }
}
impl CalendarStore {
    fn imported(&self) -> Result<(Snapshot, Option<String>)> {
        let dir = self.dir(false)?;
        let bytes = self.bytes(&dir.join("events.json"))?;
        let snapshot: Snapshot = match &bytes {
            None => Snapshot::default(),
            Some(bytes) => {
                let value: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| CalendarError::Storage)?;
                if value["schema_version"] != 1 {
                    return Err(CalendarError::Storage.into());
                }
                serde_json::from_value(value).map_err(|_| CalendarError::Storage)?
            }
        };
        // A failure belongs to the exact snapshot it observed. A successful
        // replacement changes generation even if events and clock were identical.
        let error = if let Some(error) = self.bytes(&dir.join("last_error.json"))? {
            let error: serde_json::Value =
                serde_json::from_slice(&error).map_err(|_| CalendarError::Storage)?;
            if error["snapshot_sha256"] == sha256_hex(&bytes.unwrap_or_default()) {
                error["code"].as_str().map(str::to_string)
            } else {
                None
            }
        } else {
            None
        };
        Ok((snapshot, error))
    }
}
impl Calendar for CalendarStore {
    fn sources(&self) -> Result<CalendarSources> {
        Ok(self.local_state()?.sources)
    }
    fn change_local(&self, change: &LocalChange) -> Result<Option<String>> {
        change.validate()?;
        let store = self.local_store()?;
        let dir = store.dir(true)?;
        safe_components(&dir.join("library.writer.lock"))?;
        safe_components(&dir.join("library.writer.owner.json"))?;
        let _lock = WriterLock::acquire(&dir)?;
        let mut state = self.local_state()?;
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| CalendarError::Storage)?
                .as_nanos()
        );
        let key = state.apply(change, format!("local-{}", sha256_hex(nonce.as_bytes())))?;
        let bytes = serde_json::to_vec(&state).map_err(|_| CalendarError::Storage)?;
        if bytes.len() > 16 * 1_048_576 {
            return Err(CalendarError::TooLarge.into());
        }
        safe_components(&dir.join("local.json"))?;
        write_atomic(&dir.join("local.json"), &bytes).map_err(|_| CalendarError::Storage)?;
        Ok(key)
    }
    fn load(&self) -> Result<(Snapshot, Option<String>)> {
        let local = self.local_state()?;
        let (mut snapshot, error) = if local.sources.external {
            self.imported()?
        } else {
            (Snapshot::default(), None)
        };
        if local.sources.local {
            snapshot.events.extend(local.events);
        }
        snapshot.events.sort_by(|a, b| {
            office_core::domain::calendar::parse_time(&a.start)
                .ok()
                .cmp(&office_core::domain::calendar::parse_time(&b.start).ok())
                .then(a.key.cmp(&b.key))
        });
        Ok((snapshot, if local.sources.external { error } else { None }))
    }
    fn replace(&self, snapshot: &Snapshot) -> Result<()> {
        let dir = self.dir(true)?;
        safe_components(&dir.join("library.writer.lock"))?;
        safe_components(&dir.join("library.writer.owner.json"))?;
        let _lock = WriterLock::acquire(&dir)?;
        // Check version before mutation, including future versions of persisted state.
        self.imported()?;
        let mut value = serde_json::to_value(snapshot).map_err(|_| CalendarError::Storage)?;
        value["schema_version"] = json!(1);
        value["generation"] = json!(format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| CalendarError::Storage)?
                .as_nanos()
        ));
        let bytes = serde_json::to_vec(&value).map_err(|_| CalendarError::Storage)?;
        if bytes.len() > 16 * 1_048_576 {
            return Err(CalendarError::TooLarge.into());
        }
        safe_components(&dir.join("events.json"))?;
        write_atomic(&dir.join("events.json"), &bytes).map_err(|_| CalendarError::Storage)?;
        Ok(())
    }
    fn record_error(&self, code: &str) -> Result<()> {
        let dir = self.dir(true)?;
        safe_components(&dir.join("library.writer.lock"))?;
        safe_components(&dir.join("library.writer.owner.json"))?;
        let _lock = WriterLock::acquire(&dir)?;
        self.imported()?;
        let value = json!({"snapshot_sha256":self.hash(&dir)?,"code":code});
        safe_components(&dir.join("last_error.json"))?;
        write_atomic(&dir.join("last_error.json"), value.to_string().as_bytes())
            .map_err(|_| CalendarError::Storage)?;
        Ok(())
    }
    fn read_ics(&self, relative: &str) -> Result<String> {
        let path = Path::new(relative);
        if path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
            || relative.is_empty()
            || path.extension().and_then(|e| e.to_str()) != Some("ics")
        {
            return Err(CalendarError::Invalid.into());
        }
        let root = self
            .workspace
            .canonicalize()
            .map_err(|_| CalendarError::Storage)?;
        let path = root.join(path);
        safe_components(&path)?;
        let resolved = path.canonicalize().map_err(|_| CalendarError::Invalid)?;
        if !resolved.starts_with(&root) {
            return Err(CalendarError::Storage.into());
        }
        let file = fs::File::open(path).map_err(|_| CalendarError::Invalid)?;
        if !file
            .metadata()
            .map_err(|_| CalendarError::Invalid)?
            .is_file()
        {
            return Err(CalendarError::Invalid.into());
        }
        let mut bytes = Vec::new();
        file.take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| CalendarError::Invalid)?;
        if bytes.len() > 1_048_576 {
            return Err(CalendarError::TooLarge.into());
        }
        String::from_utf8(bytes).map_err(|_| CalendarError::Invalid.into())
    }
}
