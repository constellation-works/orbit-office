//! The workspace's common error type for shared mechanisms.
//!
//! Domain crates keep their own typed errors and translate this one at
//! their boundary (STD-02 R12); callers branch on the variant or on
//! [`Error::code`], never on the message (STD-02 R10).

use std::fmt;
use std::path::PathBuf;

/// Bounded captures retained from a subprocess that timed out or hit a cap.
///
/// Callers can inspect the captured bytes through the typed fields, while
/// [`fmt::Debug`] prints only lengths and flags so formatting an error never
/// embeds raw child output or secrets in logs or diagnostics.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProcessCapture {
    /// Captured stdout (or the interleaved log when run in combined mode).
    pub stdout: cw_supervise::Captured,
    /// Captured stderr (empty and complete when run in combined mode).
    pub stderr: cw_supervise::Captured,
    /// Whether `cw-supervise` verified the group sweep and leader reap.
    /// On macOS this is `false` whenever the leader exited before the sweep.
    pub swept: bool,
}

impl fmt::Debug for ProcessCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessCapture")
            .field("stdout_len", &self.stdout.bytes.len())
            .field("stdout_truncated", &self.stdout.truncated)
            .field("stdout_complete", &self.stdout.complete)
            .field("stderr_len", &self.stderr.bytes.len())
            .field("stderr_truncated", &self.stderr.truncated)
            .field("stderr_complete", &self.stderr.complete)
            .field("swept", &self.swept)
            .finish()
    }
}

/// Everything a shared mechanism can fail with.
///
/// `#[non_exhaustive]`: this is a public boundary (STD-02 R11), so adding
/// a variant is not a breaking change.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The child was never started, so it could not perform an operation.
    #[error("cannot start subprocess {}", program.display())]
    ProcessSpawn {
        /// Executable supplied to the supervisor.
        program: PathBuf,
        /// Failure returned by process creation.
        #[source]
        source: std::io::Error,
    },
    /// A supervised child exceeded its deadline.
    #[error("subprocess exceeded its deadline")]
    ProcessTimeout {
        /// Output captured before the child was stopped; omitted from Display/Debug.
        capture: ProcessCapture,
    },
    /// A supervised child exceeded its per-stream capture budget.
    #[error("subprocess exceeded its output capture limit")]
    ProcessOutputLimit {
        /// Output captured before the child was stopped; omitted from Display/Debug.
        capture: ProcessCapture,
    },
    /// The platform has no supported process group supervisor.
    #[error("subprocess supervision requires Linux or macOS")]
    ProcessUnsupported,
    /// An id or path component is not a single safe path component.
    #[error("{kind} id must be a single path component without separators or traversal: {value}")]
    UnsafePath {
        /// What was being validated ("source", "artifact", …).
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// A filesystem operation failed.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being attempted, as a verb phrase.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// The operating-system error.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// A stable `snake_case` code for this failure (STD-01 R19).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProcessSpawn { .. } => "process_spawn_failed",
            Self::ProcessTimeout { .. } => "process_timeout",
            Self::ProcessOutputLimit { .. } => "process_output_limit",
            Self::ProcessUnsupported => "process_unsupported",
            Self::UnsafePath { .. } => "unsafe_path",
            Self::Io { .. } => "io_error",
        }
    }

    /// Output captured before a timeout or output-limit stop, when present.
    #[must_use]
    pub fn process_capture(&self) -> Option<&ProcessCapture> {
        match self {
            Self::ProcessTimeout { capture } | Self::ProcessOutputLimit { capture } => {
                Some(capture)
            }
            Self::ProcessSpawn { .. }
            | Self::ProcessUnsupported
            | Self::UnsafePath { .. }
            | Self::Io { .. } => None,
        }
    }

    /// Build an [`Error::Io`] for `action` on `path`.
    pub fn io(action: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

impl From<cw_supervise::Error> for Error {
    fn from(error: cw_supervise::Error) -> Self {
        match error {
            cw_supervise::Error::Spawn { program, source } => Self::ProcessSpawn {
                program: PathBuf::from(program),
                source,
            },
            cw_supervise::Error::Unsupported => Self::ProcessUnsupported,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
