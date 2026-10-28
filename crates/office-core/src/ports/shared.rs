//! The team's shared library: its local clone, git transport and sync state.
use crate::Result;
use crate::domain::sharing::sync::SyncState;
use crate::domain::sharing::{SharedSettings, SharingError};

/// What a tree entry is. Only regular files are ever read or indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedFileKind {
    /// A regular file (mode 100644 or 100755).
    Regular,
    /// A symbolic link.
    Symlink,
    /// A submodule or any other special entry.
    Special,
}

/// One entry of the shared branch's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedFile {
    /// Clone-relative path, forward slashes.
    pub path: String,
    /// Entry kind.
    pub kind: SharedFileKind,
    /// The entry's object id.
    pub blob: String,
}

/// The shared branch as fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedSnapshot {
    /// The branch head; `None` when the remote has no such branch yet.
    pub head: Option<String>,
    /// Every tree entry, in path order.
    pub files: Vec<SharedFile>,
}

/// One commit to build on `base` and push.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushRequest {
    /// The fetched head this commit builds on (`None` for an empty remote).
    pub base: Option<String>,
    /// Files to write, clone-relative, with their exact bytes.
    pub writes: Vec<(String, Vec<u8>)>,
    /// Clone-relative paths to remove.
    pub removes: Vec<String>,
    /// The commit message.
    pub message: String,
}

/// The remote's answer to a push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// Accepted; the new branch as pushed.
    Pushed(SharedSnapshot),
    /// Rejected as not a fast-forward.
    Rejected,
}

/// The shared clone under `{{plugin_state}}/shared/`. Every git command runs
/// through the bounded supervisor with the transport policy in
/// [`crate::domain::sharing::sync`]. Errors never carry git's output.
pub trait SharedRepository {
    /// Hold the clone exclusively for the rest of this call, or refuse
    /// with `library_writer_busy`.
    fn lock(&self) -> Result<()>;
    /// The recorded sync state, if any.
    fn state(&self) -> Result<Option<SyncState>>;
    /// Replace the sync state atomically, owner-only.
    fn save_state(&self, state: &SyncState) -> Result<()>;
    /// Delete the clone and the sync state, so the next fetch starts fresh.
    fn reset(&self) -> Result<()>;
    /// Create the clone when missing and fetch the branch, without touching
    /// the working tree.
    fn fetch(&self, settings: &SharedSettings) -> Result<SharedSnapshot>;
    /// One object's bytes, bounded.
    fn read(&self, blob: &str) -> Result<Vec<u8>>;
    /// Make the working tree exactly `head` (empty for `None`), discarding
    /// local commits and files.
    fn checkout(&self, head: Option<&str>) -> Result<()>;
    /// Commit `request` with the user's git identity and push it.
    fn push(&self, settings: &SharedSettings, request: &PushRequest) -> Result<PushOutcome>;
}

/// Refusing adapter for call contexts without plugin state.
pub struct UnavailableShared;
impl SharedRepository for UnavailableShared {
    fn lock(&self) -> Result<()> {
        Err(unavailable())
    }
    fn state(&self) -> Result<Option<SyncState>> {
        Err(unavailable())
    }
    fn save_state(&self, _: &SyncState) -> Result<()> {
        Err(unavailable())
    }
    fn reset(&self) -> Result<()> {
        Err(unavailable())
    }
    fn fetch(&self, _: &SharedSettings) -> Result<SharedSnapshot> {
        Err(unavailable())
    }
    fn read(&self, _: &str) -> Result<Vec<u8>> {
        Err(unavailable())
    }
    fn checkout(&self, _: Option<&str>) -> Result<()> {
        Err(unavailable())
    }
    fn push(&self, _: &SharedSettings, _: &PushRequest) -> Result<PushOutcome> {
        Err(unavailable())
    }
}
fn unavailable() -> crate::Error {
    SharingError::Storage {
        operation: "clone access",
    }
    .into()
}
