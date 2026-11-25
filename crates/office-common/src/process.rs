//! Thin workspace adapter over [`cw_supervise`] for bounded synchronous
//! subprocess supervision.
//!
//! Every child runs through [`cw_supervise::Run`], which owns process-group
//! isolation, stdio setup, the polite-then-forced stop on timeout, the group
//! sweep on every exit path, and bounded concurrent pipe drains. Every call
//! states an explicit deadline, an explicit per-stream or combined [`Cap`]
//! policy (`Cap::kill` vs `Cap::truncate`), and a cleared allowlisted
//! environment built with [`env::allow`] or [`env::Allow`].

use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::Duration;

pub use cw_supervise::{Cap, Captured, End, Outcome, Overflow, env};

use crate::{Error, ProcessCapture, Result};

/// Bounded process captures returned when a child exits on its own.
/// Do not embed raw child output in diagnostics.
pub struct CapturedOutput {
    /// Actual exit status, after the process group has been swept and reaped.
    pub status: ExitStatus,
    /// Captured standard output, within the caller's stdout cap.
    pub stdout: Vec<u8>,
    /// Captured standard error, within the caller's stderr cap.
    pub stderr: Vec<u8>,
    /// Whether `cw-supervise` verified the group sweep and leader reap.
    /// On macOS this is `false` whenever the leader exited before the sweep.
    pub swept: bool,
}

/// Combined bounded log, retained even when a timeout or output cap stops the child.
pub struct LoggedOutput {
    /// Actual exit status, absent when supervision terminated execution.
    pub status: Option<ExitStatus>,
    /// Combined stdout and stderr, at most the caller's cap.
    pub bytes: Vec<u8>,
    /// True when the wall-clock deadline was reached.
    pub timed_out: bool,
    /// True when at least one byte exceeded the combined cap.
    pub truncated: bool,
    /// Whether `cw-supervise` verified the group sweep and leader reap.
    /// On macOS this is `false` whenever the leader exited before the sweep.
    pub swept: bool,
}

/// Supervise `command` through [`cw_supervise::Run`] with explicit deadline
/// and per-stream caps, retaining both streams on exit, timeout and overflow.
///
/// A timeout returns [`Error::ProcessTimeout`] carrying the captured output;
/// a `Cap::kill` overflow returns [`Error::ProcessOutputLimit`] carrying the
/// captured prefix. Spawn failures return [`Error::ProcessSpawn`] so
/// `ErrorKind::NotFound` stays distinguishable.
pub fn run_captured(
    command: Command,
    deadline: Duration,
    stdout_cap: Cap,
    stderr_cap: Cap,
) -> Result<CapturedOutput> {
    let outcome = cw_supervise::Run::new(command, deadline)
        .stdout(stdout_cap)
        .stderr(stderr_cap)
        .run()?;
    match outcome.end {
        End::Exited(status) => Ok(CapturedOutput {
            status,
            stdout: outcome.stdout.bytes,
            stderr: outcome.stderr.bytes,
            swept: outcome.swept,
        }),
        End::TimedOut => Err(Error::ProcessTimeout {
            capture: ProcessCapture {
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                swept: outcome.swept,
            },
        }),
        End::OutputLimit => Err(Error::ProcessOutputLimit {
            capture: ProcessCapture {
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                swept: outcome.swept,
            },
        }),
    }
}

/// Supervise `command` through [`cw_supervise::Run`] with explicit deadline
/// and per-stream caps, decoding stdout as UTF-8.
///
/// Timeout and `Cap::kill` overflow preserve their typed errors and carry the
/// captured output; spawn failures return [`Error::ProcessSpawn`].
pub fn run(
    command: Command,
    root: &Path,
    deadline: Duration,
    stdout_cap: Cap,
    stderr_cap: Cap,
) -> Result<(ExitStatus, String)> {
    let output = run_captured(command, deadline, stdout_cap, stderr_cap)?;
    let text = String::from_utf8(output.stdout)
        .map_err(|e| Error::io("decode subprocess output", root, std::io::Error::other(e)))?;
    Ok((output.status, text))
}

/// Supervise `command` through [`cw_supervise::Run::combined`] with explicit
/// deadline and combined cap, retaining the interleaved log on exit, timeout
/// and overflow.
pub fn run_logged(command: Command, deadline: Duration, cap: Cap) -> Result<LoggedOutput> {
    let outcome = cw_supervise::Run::new(command, deadline)
        .combined(cap)
        .run()?;
    let (status, timed_out, truncated) = match outcome.end {
        End::Exited(status) => (Some(status), false, outcome.stdout.truncated),
        End::TimedOut => (None, true, outcome.stdout.truncated),
        End::OutputLimit => (None, false, true),
    };
    Ok(LoggedOutput {
        status,
        bytes: outcome.stdout.bytes,
        timed_out,
        truncated,
        swept: outcome.swept,
    })
}
