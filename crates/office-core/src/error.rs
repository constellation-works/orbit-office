//! The crate's one error type.
//!
//! Callers branch on the variant (or on [`Error::code`]), never on the message
//! (STD-02 R10). Each failure mode gets its own variant and message, and a
//! message asserts only causes the code actually checked (STD-02 R26).

/// Everything a protocol or domain operation can fail with.
///
/// `#[non_exhaustive]`: this is the library's public boundary (STD-02 R11),
/// so adding a variant is not a breaking change.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No adapter was supplied for Library operations.
    #[error("Library access is unavailable in this call context")]
    LibraryUnavailable,
    /// The adapter failed; the code is preserved for tool envelopes.
    #[error("Library operation ({code}) failed: {message}")]
    Library {
        /// Stable adapter error code.
        code: &'static str,
        /// Safe diagnostic, without document contents or secrets.
        message: String,
    },
    /// A Library setting was refused at load time.
    #[error("invalid Library setting {key}: {reason}")]
    InvalidSetting {
        /// Dotted configuration key.
        key: String,
        /// Expected type, range or supported syntax; never the raw value.
        reason: &'static str,
    },
    /// The request envelope is not JSON, or is not a JSON object.
    #[error("request is not valid: {0}")]
    InvalidRequest(String),
    /// The envelope's `schema_version` is not one this backend speaks.
    #[error("unsupported schema_version: {0}")]
    UnsupportedVersion(String),
    /// The tool name is outside `office.*`, or the verb is not implemented.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// Tool input is not an object, or carries keys the tool does not accept.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl Error {
    /// A stable `snake_case` code for this failure, for the response envelope
    /// (STD-01 R19). Adding a code is fine; changing one is a breaking change.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::LibraryUnavailable => "library_unavailable",
            Self::Library { code, .. } => code,
            Self::InvalidSetting { .. } => "invalid_library_setting",
            Self::InvalidRequest(_) => "invalid_request",
            Self::UnsupportedVersion(_) => "unsupported_schema_version",
            Self::UnknownTool(_) => "unknown_tool",
            Self::InvalidArgument(_) => "invalid_argument",
        }
    }

    /// Whether retrying the same request could succeed. Protocol refusals
    /// are deterministic, so none of them are retryable.
    #[must_use]
    pub fn retryable(&self) -> bool {
        false
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
