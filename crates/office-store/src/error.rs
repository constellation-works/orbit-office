//! The crate's one error type.
//!
//! Callers branch on the variant (or on [`Error::code`]), never on the
//! message (STD-02 R10). Each failure mode gets its own variant, and a
//! message asserts only causes the code actually checked, carrying the
//! resolved path that decided the outcome (STD-02 R26).

use std::path::PathBuf;

/// Everything a Library store operation can fail with.
///
/// `#[non_exhaustive]`: this is the library's public boundary
/// (STD-02 R11), so adding a variant is not a breaking change.
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
    /// A Library file exists but is not a document this build can parse.
    #[error("{} is not a valid Library document: {reason}", path.display())]
    Corrupt {
        /// The document file.
        path: PathBuf,
        /// What the parser rejected.
        reason: String,
    },
    /// A SQLite operation failed.
    #[error("index database {}: {detail}", path.display())]
    Sqlite {
        /// The database file (or `<in-memory>`).
        path: PathBuf,
        /// The operation and SQLite's message.
        detail: String,
        /// The underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// The index was written by a newer build; refuse rather than rewrite
    /// state this build does not understand.
    #[error("index database {} uses schema {found}, but this build reads schema {supported} or older; upgrade office", path.display())]
    NewerSchema {
        /// The database file.
        path: PathBuf,
        /// The schema version the file declares.
        found: u32,
        /// The newest schema this build understands.
        supported: u32,
    },
}

impl Error {
    /// A stable `snake_case` code for this failure (STD-01 R19). Adding a
    /// code is fine; changing one is a breaking change.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsafePath { .. } => "unsafe_path",
            Self::Io { .. } => "io_error",
            Self::Corrupt { .. } => "library_document_corrupt",
            Self::Sqlite { .. } => "index_sqlite_error",
            Self::NewerSchema { .. } => "index_schema_too_new",
        }
    }

    pub(crate) fn io(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }

    pub(crate) fn sqlite(path: &std::path::Path, phase: &str, source: rusqlite::Error) -> Self {
        Self::Sqlite {
            path: path.to_path_buf(),
            detail: format!("{phase}: {source}"),
            source,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
