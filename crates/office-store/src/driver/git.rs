//! The Library's own repository; no global config, hooks, shell or signing.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::contracts::Revision;
use crate::{Error, Result};
use office_common::process;

pub(crate) struct Git {
    root: PathBuf,
    state: PathBuf,
}

impl Git {
    pub(crate) fn new(root: &Path, state: &Path) -> Self {
        Self {
            root: root.into(),
            state: state.into(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<(bool, String)> {
        let mut command = Command::new("git");
        command
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.state)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .env("GIT_LITERAL_PATHSPECS", "1")
            .args([
                "--no-pager",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
            ])
            .args(args);
        let (status, text) = process::run(&mut command, &self.root)?;
        Ok((status.success(), text))
    }

    fn checked(&self, operation: &'static str, args: &[&str]) -> Result<String> {
        let (ok, text) = self.run(args)?;
        // Git output may contain document contents, credentials or local config;
        // never embed it in a tool error. The typed operation identifies failure.
        if !ok {
            return Err(Error::Git {
                operation,
                root: self.root.clone(),
            });
        }
        Ok(text)
    }

    /// Whether a usable `git` is on PATH. A missing binary is its own
    /// typed refusal (`git_missing`), distinct from a git failure.
    pub(crate) fn ensure_available(cwd: &Path) -> Result<()> {
        let mut command = Command::new("git");
        command
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["--version"]);
        match process::run(&mut command, cwd) {
            Ok((status, _)) if status.success() => Ok(()),
            Ok(_) => Err(Error::GitMissing),
            Err(office_common::Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Err(Error::GitMissing)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Create this root's own repository. The root must already exist.
    pub(crate) fn init(&self) -> Result<()> {
        self.checked("initialize repository", &["init", "--quiet"])?;
        Ok(())
    }

    /// Stage and commit only `paths` under `message`, with the fixed
    /// Library author. No commit is made when the paths are unchanged.
    pub(crate) fn commit_created(&self, paths: &[String], message: &str) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let mut add: Vec<&str> = vec!["add", "--force", "--"];
        add.extend(paths.iter().map(String::as_str));
        self.checked("stage Library scaffold", &add)?;
        let mut status: Vec<&str> = vec!["status", "--porcelain", "--"];
        status.extend(paths.iter().map(String::as_str));
        let changes = self.checked("inspect Library scaffold", &status)?;
        if changes.trim().is_empty() {
            return Ok(false);
        }
        let mut commit: Vec<&str> = vec![
            "-c",
            "user.name=orbit-office",
            "-c",
            "user.email=office@orbit.local",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "--author=orbit-office <office@orbit.local>",
            "--only",
            "-m",
            message,
            "--",
        ];
        commit.extend(paths.iter().map(String::as_str));
        self.checked("commit Library scaffold", &commit)?;
        Ok(true)
    }

    pub(crate) fn is_repository(&self) -> Result<bool> {
        // A .git directory *at this root* prevents discovery of a parent repo.
        let metadata = match std::fs::symlink_metadata(self.root.join(".git")) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(Error::io("inspect Library repository", &self.root, e)),
        };
        if !metadata.is_dir() {
            return Ok(false);
        }
        let top = self.checked("resolve repository", &["rev-parse", "--show-toplevel"])?;
        let actual = Path::new(top.trim())
            .canonicalize()
            .map_err(|e| Error::io("resolve repository root", &self.root, e))?;
        // git reports the resolved path (macOS: /private/var for /var), so
        // compare canonical forms.
        let expected = self
            .root
            .canonicalize()
            .map_err(|e| Error::io("resolve Library root", &self.root, e))?;
        Ok(actual == expected)
    }

    pub(crate) fn commit(&self, relative: &str, token: &str) -> Result<()> {
        // A replay after a crash just after commit must not add a duplicate.
        let (has_head, _) = self.run(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        if has_head {
            let message = self.checked("read latest commit", &["log", "-1", "--format=%B"])?;
            if message
                .lines()
                .any(|line| line == format!("office-write: {token}"))
            {
                return Ok(());
            }
        }
        self.checked("stage document", &["add", "--force", "--", relative])?;
        let message = format!("Update {relative}\n\noffice-write: {token}");
        // --only isolates this write from unrelated staged files, even originals
        // that somebody staged before our call. Per-command identity never edits
        // either the Library's config or the user's global config.
        self.checked(
            "commit document",
            &[
                "-c",
                "user.name=orbit-office",
                "-c",
                "user.email=office@orbit.local",
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "--author=orbit-office <office@orbit.local>",
                "--only",
                "--allow-empty",
                "-m",
                &message,
                "--",
                relative,
            ],
        )?;
        Ok(())
    }

    /// The current commit, or `None` before the first commit.
    pub(crate) fn head(&self) -> Result<Option<String>> {
        let (has_head, text) = self.run(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
        let commit = text.trim();
        if !has_head || commit.is_empty() {
            return Ok(None);
        }
        Ok(Some(commit.to_string()))
    }

    pub(crate) fn history(&self, relative: &str) -> Result<Vec<Revision>> {
        let (has_head, _) = self.run(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        if !has_head {
            return Ok(Vec::new());
        }
        let text = self.checked(
            "read document history",
            &[
                "log",
                "--format=%H%x00%an%x00%ae%x00%aI%x00%s",
                "--",
                relative,
            ],
        )?;
        text.lines()
            .map(|line| {
                let parts: Vec<_> = line.splitn(5, '\0').collect();
                if parts.len() != 5 {
                    return Err(Error::Git {
                        operation: "decode history",
                        root: self.root.clone(),
                    });
                }
                Ok(Revision {
                    commit: parts[0].into(),
                    author: parts[1].into(),
                    email: parts[2].into(),
                    authored_at: parts[3].into(),
                    message: parts[4].into(),
                })
            })
            .collect()
    }
}
