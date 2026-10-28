//! An in-memory shared library remote and clone. Clones share one remote, so
//! a test can play a colleague pushing between syncs.
use office_common::fs::hash::sha256_hex;
use office_core::Result;
use office_core::domain::sharing::sync::{SyncState, remote_host};
use office_core::domain::sharing::{SharedSettings, SharingError};
use office_core::ports::{
    PushOutcome, PushRequest, SharedFile, SharedFileKind, SharedRepository, SharedSnapshot,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Default)]
struct Inner {
    head: Option<String>,
    files: BTreeMap<String, (SharedFileKind, String)>,
    objects: BTreeMap<String, Vec<u8>>,
    commits: usize,
    state: Option<SyncState>,
    checked_out: Option<Option<String>>,
    reject: usize,
    unreachable: bool,
    busy: bool,
    pushes: Vec<PushRequest>,
    fetches: usize,
    resets: usize,
}

/// Fake shared repository. Blob ids are SHA-256 of the bytes.
#[derive(Clone, Default)]
pub struct FakeShared {
    inner: Rc<RefCell<Inner>>,
}

impl FakeShared {
    fn object(inner: &mut Inner, bytes: &[u8]) -> String {
        let blob = sha256_hex(bytes);
        inner.objects.insert(blob.clone(), bytes.to_vec());
        blob
    }

    fn commit(inner: &mut Inner) {
        inner.commits += 1;
        inner.head = Some(format!("{:040x}", inner.commits));
    }

    /// A colleague commits one regular file directly to the remote.
    pub fn put_remote(&self, path: &str, bytes: &[u8]) {
        let mut inner = self.inner.borrow_mut();
        let blob = Self::object(&mut inner, bytes);
        inner
            .files
            .insert(path.into(), (SharedFileKind::Regular, blob));
        Self::commit(&mut inner);
    }

    /// A colleague commits a symbolic link (its target as the blob).
    pub fn put_remote_symlink(&self, path: &str, target: &str) {
        let mut inner = self.inner.borrow_mut();
        let blob = Self::object(&mut inner, target.as_bytes());
        inner
            .files
            .insert(path.into(), (SharedFileKind::Symlink, blob));
        Self::commit(&mut inner);
    }

    /// The remote's regular files and their bytes.
    #[must_use]
    pub fn remote_files(&self) -> BTreeMap<String, Vec<u8>> {
        let inner = self.inner.borrow();
        inner
            .files
            .iter()
            .filter(|(_, (kind, _))| *kind == SharedFileKind::Regular)
            .filter_map(|(path, (_, blob))| {
                inner
                    .objects
                    .get(blob)
                    .map(|bytes| (path.clone(), bytes.clone()))
            })
            .collect()
    }

    /// Commits on the remote, including colleagues'.
    #[must_use]
    pub fn commits(&self) -> usize {
        self.inner.borrow().commits
    }

    /// Every push request received, accepted or rejected.
    #[must_use]
    pub fn pushes(&self) -> Vec<PushRequest> {
        self.inner.borrow().pushes.clone()
    }

    /// Fetches performed.
    #[must_use]
    pub fn fetches(&self) -> usize {
        self.inner.borrow().fetches
    }

    /// Clone resets performed.
    #[must_use]
    pub fn resets(&self) -> usize {
        self.inner.borrow().resets
    }

    /// The recorded sync state.
    #[must_use]
    pub fn sync_state(&self) -> Option<SyncState> {
        self.inner.borrow().state.clone()
    }

    /// The last checkout, if any (`Some(None)` for an empty working tree).
    #[must_use]
    pub fn checked_out(&self) -> Option<Option<String>> {
        self.inner.borrow().checked_out.clone()
    }

    /// Reject the next `count` pushes as not fast-forward.
    pub fn reject_pushes(&self, count: usize) {
        self.inner.borrow_mut().reject = count;
    }

    /// Make every fetch and push fail as unreachable.
    pub fn set_unreachable(&self, unreachable: bool) {
        self.inner.borrow_mut().unreachable = unreachable;
    }

    /// Make the lock report another running sync.
    pub fn set_busy(&self, busy: bool) {
        self.inner.borrow_mut().busy = busy;
    }

    fn snapshot(inner: &Inner) -> SharedSnapshot {
        SharedSnapshot {
            head: inner.head.clone(),
            files: inner
                .files
                .iter()
                .map(|(path, (kind, blob))| SharedFile {
                    path: path.clone(),
                    kind: *kind,
                    blob: blob.clone(),
                })
                .collect(),
        }
    }
}

impl SharedRepository for FakeShared {
    fn lock(&self) -> Result<()> {
        if self.inner.borrow().busy {
            return Err(SharingError::WriterBusy.into());
        }
        Ok(())
    }
    fn state(&self) -> Result<Option<SyncState>> {
        Ok(self.inner.borrow().state.clone())
    }
    fn save_state(&self, state: &SyncState) -> Result<()> {
        self.inner.borrow_mut().state = Some(state.clone());
        Ok(())
    }
    fn reset(&self) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner.state = None;
        inner.checked_out = None;
        inner.resets += 1;
        Ok(())
    }
    fn fetch(&self, settings: &SharedSettings) -> Result<SharedSnapshot> {
        let mut inner = self.inner.borrow_mut();
        if inner.unreachable {
            return Err(SharingError::RemoteUnreachable {
                host: remote_host(&settings.remote),
            }
            .into());
        }
        inner.fetches += 1;
        Ok(Self::snapshot(&inner))
    }
    fn read(&self, blob: &str) -> Result<Vec<u8>> {
        self.inner
            .borrow()
            .objects
            .get(blob)
            .cloned()
            .ok_or_else(|| SharingError::Storage { operation: "read" }.into())
    }
    fn checkout(&self, head: Option<&str>) -> Result<()> {
        self.inner.borrow_mut().checked_out = Some(head.map(str::to_string));
        Ok(())
    }
    fn push(&self, settings: &SharedSettings, request: &PushRequest) -> Result<PushOutcome> {
        let mut inner = self.inner.borrow_mut();
        if inner.unreachable {
            return Err(SharingError::RemoteUnreachable {
                host: remote_host(&settings.remote),
            }
            .into());
        }
        inner.pushes.push(request.clone());
        if inner.reject > 0 || request.base != inner.head {
            inner.reject = inner.reject.saturating_sub(1);
            return Ok(PushOutcome::Rejected);
        }
        for (path, bytes) in &request.writes {
            let blob = Self::object(&mut inner, bytes);
            inner
                .files
                .insert(path.clone(), (SharedFileKind::Regular, blob));
        }
        for path in &request.removes {
            inner.files.remove(path);
        }
        Self::commit(&mut inner);
        Ok(PushOutcome::Pushed(Self::snapshot(&inner)))
    }
}
