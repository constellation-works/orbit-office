//! The team's shared library clone under plugin state: git transport through
//! the bounded supervisor, an exclusive sync lock and atomic sync state.
//!
//! The working tree is only ever replaced by `checkout`; commits are built
//! with plumbing from the index, so nothing is written through a path that
//! an untrusted upstream tree could have turned into a symlink.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use office_common::fs::atomic::write_atomic;
use office_common::process;
use office_core::Result;
use office_core::domain::sharing::SharedSettings;
use office_core::domain::sharing::SharingError;
use office_core::domain::sharing::sync::{
    GIT_DEADLINE_SECONDS, GIT_ENVIRONMENT, GIT_OVERRIDES, GIT_TERMINAL_PROMPT, SyncState,
    remote_host,
};
use office_core::ports::{
    PushOutcome, PushRequest, SharedFile, SharedFileKind, SharedRepository, SharedSnapshot,
};

/// Largest object read from the shared repository.
const OBJECT_LIMIT: u64 = 16 * 1_048_576;
/// Largest tree listing or sync state accepted.
const LISTING_LIMIT: u64 = 32 * 1_048_576;
/// Paths per `hash-object` or `update-index` invocation.
const BATCH: usize = 200;
/// The local ref the fetched branch is kept under.
const FETCHED: &str = "refs/remotes/origin/";

/// Adapter state for one call, rooted only in host-supplied paths.
pub struct SharedStore {
    state: PathBuf,
    environment: Vec<(String, OsString)>,
    lock: RefCell<Option<File>>,
}

/// The child environment: exactly the allowlisted variables that are set,
/// plus `GIT_TERMINAL_PROMPT=0` (STD-05@1).
#[must_use]
pub fn git_environment(lookup: impl Fn(&str) -> Option<OsString>) -> Vec<(String, OsString)> {
    let mut environment: Vec<_> = GIT_ENVIRONMENT
        .iter()
        .filter_map(|name| lookup(name).map(|value| ((*name).to_string(), value)))
        .collect();
    environment.push((
        GIT_TERMINAL_PROMPT.0.to_string(),
        GIT_TERMINAL_PROMPT.1.into(),
    ));
    environment
}

/// One git command in `root` with a cleared environment, the transport
/// policy overrides and this adapter's local safety settings.
#[must_use]
pub fn git_command(root: &Path, environment: &[(String, OsString)], args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root).env_clear();
    for (name, value) in environment {
        command.env(name, value);
    }
    command.arg("--no-pager");
    for setting in GIT_OVERRIDES {
        command.args(["-c", setting]);
    }
    for setting in [
        "core.symlinks=false",
        "core.fsmonitor=false",
        "commit.gpgSign=false",
        "gc.auto=0",
        "maintenance.auto=false",
    ] {
        command.args(["-c", setting]);
    }
    command.args(args);
    command
}

/// The hook-free receive-pack for an absolute-path or `file://` remote.
const LOCAL_RECEIVE_PACK: &str = "--receive-pack=git -c core.hooksPath=/dev/null receive-pack";

fn storage(operation: &'static str) -> office_core::Error {
    SharingError::Storage { operation }.into()
}

fn unreachable(settings: &SharedSettings) -> office_core::Error {
    SharingError::RemoteUnreachable {
        host: remote_host(&settings.remote),
    }
    .into()
}

fn private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| storage("create plugin state"))
}

fn refuse_symlinks(path: &Path) -> Result<()> {
    let mut at = PathBuf::new();
    for part in path.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(storage("resolve plugin state"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("resolve plugin state")),
        }
    }
    Ok(())
}

/// A clone-relative path to commit: normal components only, never `.git`.
fn valid_repo_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\0', '\n', '\r', '\\'])
        && Path::new(path).components().all(|part| match part {
            Component::Normal(name) => !name.eq_ignore_ascii_case(".git"),
            Component::RootDir
            | Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_) => false,
        })
}

fn valid_object(id: &str) -> bool {
    (id.len() == 40 || id.len() == 64) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl SharedStore {
    /// Resolve no ambient paths and perform no I/O. `environment` is the
    /// complete child environment (see [`git_environment`]).
    #[must_use]
    pub fn new(state: PathBuf, environment: Vec<(String, OsString)>) -> Self {
        Self {
            state,
            environment,
            lock: RefCell::new(None),
        }
    }

    fn dir(&self) -> PathBuf {
        self.state.join("shared")
    }

    fn repo(&self) -> PathBuf {
        self.dir().join("repo")
    }

    fn ensure_dir(&self) -> Result<PathBuf> {
        refuse_symlinks(&self.state)?;
        private_dir(&self.state)?;
        let dir = self.dir();
        refuse_symlinks(&dir)?;
        private_dir(&dir)?;
        Ok(dir)
    }

    fn git(&self, args: &[&str], limit: u64) -> Result<process::CapturedOutput> {
        let root = self.repo();
        let mut command = git_command(&root, &self.environment, args);
        process::run_captured_with_deadline(
            &mut command,
            &root,
            Duration::from_secs(GIT_DEADLINE_SECONDS),
            limit,
        )
        .map_err(|_| storage("git command"))
    }

    /// Run a local command that must succeed; git's output never escapes.
    fn checked(&self, operation: &'static str, args: &[&str]) -> Result<String> {
        let output = self.git(args, LISTING_LIMIT)?;
        if !output.status.success() {
            return Err(storage(operation));
        }
        String::from_utf8(output.stdout).map_err(|_| storage(operation))
    }

    fn ensure_clone(&self) -> Result<()> {
        self.ensure_dir()?;
        let repo = self.repo();
        refuse_symlinks(&repo.join(".git"))?;
        match std::fs::symlink_metadata(repo.join(".git")) {
            Ok(meta) if meta.is_dir() => return Ok(()),
            Ok(_) => return Err(storage("open clone")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("open clone")),
        }
        private_dir(&repo)?;
        self.checked("create clone", &["init", "--quiet"])?;
        Ok(())
    }

    fn tree(&self, head: &str) -> Result<Vec<SharedFile>> {
        let listing = self.checked("list tree", &["ls-tree", "-r", "-z", "--full-tree", head])?;
        let mut files = Vec::new();
        for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
            let (meta, path) = entry.split_once('\t').ok_or_else(|| storage("list tree"))?;
            let mut parts = meta.split(' ');
            let (Some(mode), Some(_), Some(blob)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(storage("list tree"));
            };
            let kind = match mode {
                "100644" | "100755" => SharedFileKind::Regular,
                "120000" => SharedFileKind::Symlink,
                _ => SharedFileKind::Special,
            };
            files.push(SharedFile {
                path: path.into(),
                kind,
                blob: blob.into(),
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    fn snapshot(&self, head: Option<String>) -> Result<SharedSnapshot> {
        let files = match &head {
            Some(head) => self.tree(head)?,
            None => Vec::new(),
        };
        Ok(SharedSnapshot { head, files })
    }

    fn resolve(&self, reference: &str) -> Result<Option<String>> {
        let output = self.git(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{commit}}"),
            ],
            4096,
        )?;
        let text = String::from_utf8(output.stdout).map_err(|_| storage("resolve head"))?;
        let head = text.trim();
        Ok((output.status.success() && valid_object(head)).then(|| head.to_string()))
    }

    fn stage(&self, writes: &[(String, Vec<u8>)]) -> Result<Vec<String>> {
        let staging = self.repo().join(".git/office-staging");
        refuse_symlinks(&staging)?;
        let _ = std::fs::remove_dir_all(&staging);
        private_dir(&staging)?;
        let mut blobs = Vec::new();
        for chunk in writes.chunks(BATCH) {
            let mut names = Vec::new();
            for (index, (_, bytes)) in chunk.iter().enumerate() {
                let name = format!("{}", blobs.len() + index);
                write_atomic(&staging.join(&name), bytes).map_err(|_| storage("stage files"))?;
                names.push(format!(".git/office-staging/{name}"));
            }
            let mut args = vec!["hash-object", "-w", "--no-filters", "--"];
            args.extend(names.iter().map(String::as_str));
            let text = self.checked("stage files", &args)?;
            let hashed: Vec<_> = text.lines().map(str::to_string).collect();
            if hashed.len() != chunk.len() || !hashed.iter().all(|id| valid_object(id)) {
                return Err(storage("stage files"));
            }
            blobs.extend(hashed);
        }
        let _ = std::fs::remove_dir_all(&staging);
        Ok(blobs)
    }

    fn identity_configured(&self) -> Result<bool> {
        for key in ["user.name", "user.email"] {
            let output = self.git(&["config", "--get", key], 4096)?;
            if !output.status.success() || output.stdout.iter().all(u8::is_ascii_whitespace) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn state_path(&self) -> PathBuf {
        self.dir().join("state.json")
    }
}

impl SharedRepository for SharedStore {
    fn lock(&self) -> Result<()> {
        if self.lock.borrow().is_some() {
            return Ok(());
        }
        let path = self.ensure_dir()?.join("sync.lock");
        refuse_symlinks(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| storage("open sync lock"))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(SharingError::WriterBusy.into()),
            Err(TryLockError::Error(_)) => return Err(storage("acquire sync lock")),
        }
        *self.lock.borrow_mut() = Some(file);
        Ok(())
    }

    fn state(&self) -> Result<Option<SyncState>> {
        let path = self.state_path();
        refuse_symlinks(&path)?;
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(storage("read sync state")),
        };
        if !file.metadata().is_ok_and(|meta| meta.is_file()) {
            return Err(storage("read sync state"));
        }
        let mut bytes = Vec::new();
        file.take(LISTING_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage("read sync state"))?;
        if u64::try_from(bytes.len()).map_or(true, |len| len > LISTING_LIMIT) {
            return Err(storage("read sync state"));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| storage("read sync state"))
    }

    fn save_state(&self, state: &SyncState) -> Result<()> {
        self.ensure_dir()?;
        let path = self.state_path();
        refuse_symlinks(&path)?;
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| storage("write sync state"))?;
        write_atomic(&path, &bytes).map_err(|_| storage("write sync state"))
    }

    fn reset(&self) -> Result<()> {
        self.ensure_dir()?;
        let repo = self.repo();
        refuse_symlinks(&repo)?;
        match std::fs::remove_dir_all(&repo) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("remove clone")),
        }
        match std::fs::remove_file(self.state_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("remove sync state")),
        }
        Ok(())
    }

    fn fetch(&self, settings: &SharedSettings) -> Result<SharedSnapshot> {
        self.ensure_clone()?;
        let fetched = format!("{FETCHED}{}", settings.branch);
        let refspec = format!("+refs/heads/{}:{fetched}", settings.branch);
        let output = self.git(
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                "--",
                &settings.remote,
                &refspec,
            ],
            LISTING_LIMIT,
        )?;
        if !output.status.success() {
            // A reachable remote without the branch is an empty shared library.
            let listed = self.git(
                &[
                    "ls-remote",
                    "--heads",
                    "--",
                    &settings.remote,
                    &format!("refs/heads/{}", settings.branch),
                ],
                LISTING_LIMIT,
            )?;
            if !listed.status.success() || !listed.stdout.is_empty() {
                return Err(unreachable(settings));
            }
            let _ = self.git(&["update-ref", "-d", &fetched], 4096)?;
            return Ok(SharedSnapshot::default());
        }
        let head = self
            .resolve(&fetched)?
            .ok_or_else(|| storage("resolve head"))?;
        self.snapshot(Some(head))
    }

    fn read(&self, blob: &str) -> Result<Vec<u8>> {
        self.ensure_clone()?;
        if !valid_object(blob) {
            return Err(storage("read file"));
        }
        let output = self.git(&["cat-file", "blob", blob], OBJECT_LIMIT)?;
        if !output.status.success() {
            return Err(storage("read file"));
        }
        Ok(output.stdout)
    }

    fn checkout(&self, head: Option<&str>) -> Result<()> {
        self.ensure_clone()?;
        match head {
            Some(head) if valid_object(head) => {
                self.checked(
                    "update working tree",
                    &["checkout", "--quiet", "--force", "--detach", head],
                )?;
                self.checked("update working tree", &["clean", "-ffdxq"])?;
            }
            Some(_) => return Err(storage("update working tree")),
            None => {
                self.checked("update working tree", &["read-tree", "--empty"])?;
                let repo = self.repo();
                let entries =
                    std::fs::read_dir(&repo).map_err(|_| storage("update working tree"))?;
                for entry in entries {
                    let entry = entry.map_err(|_| storage("update working tree"))?;
                    if entry.file_name() == ".git" {
                        continue;
                    }
                    let path = entry.path();
                    let kind = entry
                        .file_type()
                        .map_err(|_| storage("update working tree"))?;
                    let removed = if kind.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    removed.map_err(|_| storage("update working tree"))?;
                }
            }
        }
        Ok(())
    }

    fn push(&self, settings: &SharedSettings, request: &PushRequest) -> Result<PushOutcome> {
        self.ensure_clone()?;
        if request
            .writes
            .iter()
            .map(|(path, _)| path)
            .chain(&request.removes)
            .any(|path| !valid_repo_path(path))
        {
            return Err(storage("build commit"));
        }
        match &request.base {
            Some(base) if valid_object(base) => {
                self.checked("build commit", &["read-tree", base])?;
            }
            Some(_) => return Err(storage("build commit")),
            None => {
                self.checked("build commit", &["read-tree", "--empty"])?;
            }
        }
        let blobs = self.stage(&request.writes)?;
        let entries: Vec<String> = request
            .writes
            .iter()
            .zip(&blobs)
            .map(|((path, _), blob)| format!("100644,{blob},{path}"))
            .collect();
        for chunk in entries.chunks(BATCH) {
            let mut args = vec!["update-index", "--add", "--replace"];
            for entry in chunk {
                args.extend(["--cacheinfo", entry.as_str()]);
            }
            self.checked("build commit", &args)?;
        }
        for chunk in request.removes.chunks(BATCH) {
            let mut args = vec!["update-index", "--force-remove", "--"];
            args.extend(chunk.iter().map(String::as_str));
            self.checked("build commit", &args)?;
        }
        let tree = self.checked("build commit", &["write-tree"])?;
        let tree = tree.trim();
        if !valid_object(tree) {
            return Err(storage("build commit"));
        }
        if !self.identity_configured()? {
            return Err(SharingError::IdentityMissing.into());
        }
        let mut args = vec!["commit-tree", tree];
        if let Some(base) = &request.base {
            args.extend(["-p", base.as_str()]);
        }
        args.extend(["-m", request.message.as_str()]);
        let commit = self.checked("commit", &args)?;
        let commit = commit.trim().to_string();
        if !valid_object(&commit) {
            return Err(storage("commit"));
        }
        let target = format!("{commit}:refs/heads/{}", settings.branch);
        let mut args = vec!["push", "--porcelain"];
        // Git drops `-c` overrides for local transports, so a local remote's
        // own hooks would run as this user; its receive-pack gets them too.
        if remote_host(&settings.remote) == "local" {
            args.push(LOCAL_RECEIVE_PACK);
        }
        args.extend(["--", settings.remote.as_str(), target.as_str()]);
        let output = self.git(&args, LISTING_LIMIT)?;
        let rejected = String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.starts_with("!\t"));
        if rejected {
            return Ok(PushOutcome::Rejected);
        }
        if !output.status.success() {
            return Err(unreachable(settings));
        }
        let fetched = format!("{FETCHED}{}", settings.branch);
        self.checked("record push", &["update-ref", &fetched, &commit])?;
        Ok(PushOutcome::Pushed(self.snapshot(Some(commit))?))
    }
}
