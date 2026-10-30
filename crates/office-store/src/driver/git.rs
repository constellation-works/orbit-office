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

    /// Whether `path` has any tracked file beneath it. Tracked writer
    /// state stays listed even after it is ignored, so this decides
    /// whether init must untrack it.
    pub(crate) fn has_tracked(&self, path: &str) -> Result<bool> {
        let text = self.checked("inspect tracked Library state", &["ls-files", "--", path])?;
        Ok(text.lines().any(|line| !line.trim().is_empty()))
    }

    /// Stage `add_paths` and commit them with the fixed Library author.
    /// No commit is made when the paths are unchanged. Unrelated staged
    /// files stay staged and do not enter the commit.
    pub(crate) fn commit_scaffold(
        &self,
        add_paths: &[String],
        untrack_state: bool,
        message: &str,
    ) -> Result<bool> {
        if !untrack_state {
            if add_paths.is_empty() {
                return Ok(false);
            }
            let mut add: Vec<&str> = vec!["add", "--force", "--"];
            add.extend(add_paths.iter().map(String::as_str));
            self.checked("stage Library scaffold", &add)?;
            let mut status: Vec<&str> = vec!["status", "--porcelain", "--"];
            status.extend(add_paths.iter().map(String::as_str));
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
            commit.extend(add_paths.iter().map(String::as_str));
            self.checked("commit Library scaffold", &commit)?;
            return Ok(true);
        }
        self.commit_scaffold_with_untrack(add_paths, message)
    }

    /// Stage `add_paths`, remove a tracked `.office/state` from the index
    /// without touching the working tree, and commit both together in one
    /// commit. `git commit --only -- <deleted path>` cannot commit a
    /// `--cached` deletion whose files remain on disk (it reads the
    /// working tree), and a pathspec commit ignores staged deletions of
    /// now-ignored files, so the combined commit is built in a temporary
    /// index (`read-tree`, `add`, `rm --cached`, `write-tree`,
    /// `commit-tree`, `update-ref`). Unrelated staged files live only in
    /// the real index and never enter the temporary one, so they stay
    /// staged and do not enter this commit.
    fn commit_scaffold_with_untrack(&self, add_paths: &[String], message: &str) -> Result<bool> {
        if !add_paths.is_empty() {
            let mut add: Vec<&str> = vec!["add", "--force", "--"];
            add.extend(add_paths.iter().map(String::as_str));
            self.checked("stage Library scaffold", &add)?;
        }
        self.checked(
            "untrack Library writer state",
            &["rm", "-r", "--cached", "--quiet", "--", ".office/state"],
        )?;
        let (has_head, _) = self.run(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        let temp = self.temp_index_path()?;
        let _cleanup = TempIndexGuard { path: temp.clone() };
        if has_head {
            self.checked_with_index(
                &temp,
                "stage Library scaffold base",
                &["read-tree", "HEAD"],
                &[],
            )?;
        } else {
            self.checked_with_index(
                &temp,
                "stage Library scaffold base",
                &["read-tree", "--empty"],
                &[],
            )?;
        }
        if !add_paths.is_empty() {
            let mut add: Vec<&str> = vec!["add", "--force", "--"];
            add.extend(add_paths.iter().map(String::as_str));
            self.checked_with_index(&temp, "stage Library scaffold", &add, &[])?;
        }
        self.checked_with_index(
            &temp,
            "untrack Library writer state",
            &["rm", "-r", "--cached", "--quiet", "--", ".office/state"],
            &[],
        )?;
        let tree =
            self.checked_with_index(&temp, "inspect Library scaffold", &["write-tree"], &[])?;
        if has_head {
            let head_tree = self.checked(
                "resolve Library scaffold base",
                &["rev-parse", "HEAD^{tree}"],
            )?;
            if tree.trim() == head_tree.trim() {
                return Ok(false);
            }
        }
        let tree_hash = tree.trim().to_string();
        let commit_args: Vec<&str> = if has_head {
            vec![
                "commit-tree",
                tree_hash.as_str(),
                "-p",
                "HEAD",
                "-m",
                message,
            ]
        } else {
            vec!["commit-tree", tree_hash.as_str(), "-m", message]
        };
        let author: [(&str, &str); 4] = [
            ("GIT_AUTHOR_NAME", "orbit-office"),
            ("GIT_AUTHOR_EMAIL", "office@orbit.local"),
            ("GIT_COMMITTER_NAME", "orbit-office"),
            ("GIT_COMMITTER_EMAIL", "office@orbit.local"),
        ];
        let hash =
            self.checked_with_index(&temp, "commit Library scaffold", &commit_args, &author)?;
        let hash = hash.trim();
        if hash.is_empty() {
            return Err(Error::Git {
                operation: "commit Library scaffold",
                root: self.root.clone(),
            });
        }
        self.checked("commit Library scaffold", &["update-ref", "HEAD", hash])?;
        Ok(true)
    }

    /// A unique sibling of writer state for the temporary scaffold index.
    /// It lives beside the lock files (ignored when inside the Library)
    /// and is removed after the commit attempt.
    fn temp_index_path(&self) -> Result<PathBuf> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        Ok(self.state.join(format!(
            ".tmp-scaffold-index-{}-{}.idx",
            std::process::id(),
            nanos
        )))
    }

    fn run_with_index(
        &self,
        index: &Path,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> Result<(bool, String)> {
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
            .env("GIT_INDEX_FILE", index)
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
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let (status, text) = process::run(&mut command, &self.root)?;
        Ok((status.success(), text))
    }

    fn checked_with_index(
        &self,
        index: &Path,
        operation: &'static str,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> Result<String> {
        let (ok, text) = self.run_with_index(index, args, extra_env)?;
        if !ok {
            return Err(Error::Git {
                operation,
                root: self.root.clone(),
            });
        }
        Ok(text)
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

    pub(crate) fn commit_paths(&self, paths: &[String], token: &str) -> Result<()> {
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
        let mut add = vec!["add", "--force", "--"];
        add.extend(paths.iter().map(String::as_str));
        self.checked("stage document", &add)?;
        let message = format!("Update {}\n\noffice-write: {token}", paths.join(", "));
        // --only isolates this write from unrelated staged files, even originals
        // that somebody staged before our call. Per-command identity never edits
        // either the Library's config or the user's global config.
        let mut args = vec![
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
        ];
        args.extend(paths.iter().map(String::as_str));
        self.checked("commit document", &args)?;
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

/// Removes the temporary scaffold index on scope exit. Best effort:
/// the file lives beside writer state (ignored inside the Library) and
/// a leftover never enters `git status`.
struct TempIndexGuard {
    path: PathBuf,
}

impl Drop for TempIndexGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
