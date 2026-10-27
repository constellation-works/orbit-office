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
    /// A document failed structural validation before any Library write.
    #[error("invalid {format} document: {reason}")]
    DocumentInvalid {
        /// Format being decoded.
        format: &'static str,
        /// Safe structural diagnosis, never document contents.
        reason: &'static str,
    },
    /// The optional document converter is absent from the effective PATH.
    #[error("{program} is unavailable on PATH; install poppler to capture PDF documents")]
    ConverterUnavailable {
        /// Missing executable.
        program: &'static str,
    },
    /// A converter ran and refused its input; raw output is suppressed.
    #[error("{program} refused the document (exit code {exit_code:?}); no source was written")]
    ConversionFailed {
        /// Converter executable.
        program: &'static str,
        /// Exit code, absent when terminated by a signal.
        exit_code: Option<i32>,
    },
    /// A bounded converter could not run or finish.
    #[error("{program} supervision failed: {source}")]
    ConversionProcess {
        /// Converter executable.
        program: &'static str,
        /// Structured supervisor failure, without converter output.
        source: office_common::Error,
    },
    /// A document or expanded package exceeds the documented resource bound.
    #[error("document conversion exceeds the {limit} limit")]
    ConversionLimit {
        /// Which resource was bounded.
        limit: &'static str,
    },
    /// A document id already belongs to a different Library path.
    #[error("document id {id} is already indexed at {existing_path}; refused write to {path}")]
    DocumentConflict {
        /// Conflicting id.
        id: String,
        /// Existing Library-relative path.
        existing_path: String,
        /// Requested Library-relative path.
        path: String,
    },
    /// The Library must be initialized explicitly, never as a side effect of a tool.
    #[error("Library {} is not initialized; run office.init", root.display())]
    NotInitialized {
        /// Resolved Library root.
        root: PathBuf,
    },
    /// The requested Library location is outside the granted write root.
    #[error("library_dir {requested} is outside the granted Library root {allowed}")]
    LibraryDirOutside {
        /// Requested Library location, as resolved.
        requested: PathBuf,
        /// Granted `{{workspace}}/library` root.
        allowed: PathBuf,
    },
    /// `git` is required to scaffold the Library but was not found.
    #[error("git is required to initialize the Library but was not found on PATH")]
    GitMissing,
    /// A validated setting failed at load.
    #[error(transparent)]
    Settings(#[from] office_core::Error),
    /// Lock deadline expired. The owner record is diagnostic, never authority.
    #[error("writer lock {} timed out; holder: {holder}", path.display())]
    LockTimeout {
        /// Lock file under plugin state.
        path: PathBuf,
        /// PID, acquisition time and operation label, or unavailable.
        holder: String,
    },
    /// An interrupted operation must be recovered before reads can resume.
    #[error("Library {} has a pending write; reopen it to finish recovery", root.display())]
    PendingWrite {
        /// Library root.
        root: PathBuf,
    },
    /// A journal could not be validated; never discard uncertain state.
    #[error("invalid Library write journal {}", path.display())]
    Journal {
        /// Journal file.
        path: PathBuf,
    },
    /// Git ran and rejected the operation (its potentially sensitive output is suppressed).
    #[error("git could not {operation} in Library {}", root.display())]
    Git {
        /// Attempted operation.
        operation: &'static str,
        /// Repository root.
        root: PathBuf,
    },
    /// The child exceeded its wall-clock budget.
    #[error("git exceeded its 30-second deadline")]
    ProcessTimeout,
    /// The child exceeded the captured-output budget.
    #[error("git exceeded its 512 KiB per-stream output limit")]
    ProcessOutputLimit,
    /// Supervision is not available on this platform.
    #[error("Library git supervision requires Linux or macOS")]
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
    /// The selection tray database failed. It is separate from the index.
    #[error("selections database {}: {detail}", path.display())]
    SelectionSqlite {
        /// `plugin_state/selections.sqlite`.
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
            Self::DocumentInvalid { .. } => "document_invalid",
            Self::ConverterUnavailable { .. } => "converter_unavailable",
            Self::ConversionFailed { .. } => "conversion_failed",
            Self::ConversionProcess { source, .. } => match source {
                office_common::Error::ProcessTimeout => "conversion_timeout",
                office_common::Error::ProcessOutputLimit => "conversion_output_limit",
                office_common::Error::ProcessSpawn { .. }
                | office_common::Error::ProcessUnsupported
                | office_common::Error::UnsafePath { .. }
                | office_common::Error::Io { .. } => "conversion_failed",
                _ => "conversion_failed",
            },
            Self::ConversionLimit { .. } => "conversion_limit",
            Self::DocumentConflict { .. } => "library_document_conflict",
            Self::LibraryDirOutside { .. } => "library_dir_outside_workspace",
            Self::GitMissing => "git_missing",
            Self::NotInitialized { .. } => "library_not_initialized",
            Self::Settings(error) => error.code(),
            Self::LockTimeout { .. } => "library_lock_timeout",
            Self::PendingWrite { .. } => "library_write_pending",
            Self::Journal { .. } => "library_journal_invalid",
            Self::Git { .. } => "library_git_error",
            Self::ProcessTimeout => "library_git_timeout",
            Self::ProcessOutputLimit => "library_git_output_limit",
            Self::ProcessUnsupported => "library_git_unsupported",
            Self::UnsafePath { .. } => "unsafe_path",
            Self::Io { .. } => "io_error",
            Self::Corrupt { .. } => "library_document_corrupt",
            Self::Sqlite { .. } => "index_sqlite_error",
            Self::SelectionSqlite { .. } => "selection_sqlite_error",
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

    pub(crate) fn selection_sqlite(
        path: &std::path::Path,
        phase: &str,
        source: rusqlite::Error,
    ) -> Self {
        Self::SelectionSqlite {
            path: path.to_path_buf(),
            detail: format!("{phase}: {source}"),
            source,
        }
    }
}

/// The one translator from the common crate's error (STD-02 R12):
/// shared mechanisms fail with `office_common::Error`, and this crate
/// surfaces them as its own variants, applied with `?`.
// `office_common::Error` is `#[non_exhaustive]` in its own crate, so the
// wildcard arm is required; it maps any future common failure to a
// generic I/O failure rather than dropping it (STD-02 R27 scoped allow).
#[allow(clippy::wildcard_enum_match_arm)]
impl From<office_common::Error> for Error {
    fn from(error: office_common::Error) -> Self {
        match error {
            office_common::Error::ProcessTimeout => Self::ProcessTimeout,
            office_common::Error::ProcessOutputLimit => Self::ProcessOutputLimit,
            office_common::Error::ProcessUnsupported => Self::ProcessUnsupported,
            office_common::Error::UnsafePath { kind, value } => Self::UnsafePath { kind, value },
            office_common::Error::Io {
                action,
                path,
                source,
            } => Self::Io {
                action,
                path,
                source,
            },
            other => Self::Io {
                action: "shared mechanism",
                path: PathBuf::new(),
                source: std::io::Error::other(other.to_string()),
            },
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The one translation into the pure tool-facing Library error boundary.
impl From<Error> for office_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Settings(error) => error,
            other @ (Error::DocumentConflict { .. }
            | Error::DocumentInvalid { .. }
            | Error::ConverterUnavailable { .. }
            | Error::ConversionFailed { .. }
            | Error::ConversionProcess { .. }
            | Error::ConversionLimit { .. }
            | Error::LibraryDirOutside { .. }
            | Error::GitMissing
            | Error::NotInitialized { .. }
            | Error::LockTimeout { .. }
            | Error::PendingWrite { .. }
            | Error::Journal { .. }
            | Error::Git { .. }
            | Error::ProcessTimeout
            | Error::ProcessOutputLimit
            | Error::ProcessUnsupported
            | Error::UnsafePath { .. }
            | Error::Io { .. }
            | Error::Corrupt { .. }
            | Error::Sqlite { .. }
            | Error::SelectionSqlite { .. }
            | Error::NewerSchema { .. }) => Self::Library {
                code: other.code(),
                message: other.to_string(),
            },
        }
    }
}
