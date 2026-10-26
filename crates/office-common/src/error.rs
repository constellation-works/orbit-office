//! The workspace's common error type for shared mechanisms.
//!
//! Domain crates keep their own typed errors and translate this one at
//! their boundary (STD-02 R12); callers branch on the variant or on
//! [`Error::code`], never on the message (STD-02 R10).

use std::path::PathBuf;

/// Everything a shared mechanism can fail with.
///
/// `#[non_exhaustive]`: this is a public boundary (STD-02 R11), so adding
/// a variant is not a breaking change.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
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
            Self::UnsafePath { .. } => "unsafe_path",
            Self::Io { .. } => "io_error",
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

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
