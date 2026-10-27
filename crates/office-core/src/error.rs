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
    /// No callback adapter was supplied for this call.
    #[error("Orbit callbacks are unavailable in this call context")]
    OrbitUnavailable,
    /// A write was sent but its result could not be verified. Never retry blindly.
    #[error("Orbit callback outcome is unknown; inspect Orbit before retrying")]
    OrbitOutcomeUnknown,
    /// Orbit rejected a callback; its structured code is preserved.
    #[error("Orbit refused the callback ({code})")]
    OrbitRefused {
        /// Orbit's machine-readable refusal code, without diagnostic text.
        code: String,
    },
    /// The callback channel failed; raw child output is never included.
    #[error("Orbit callback ({code}) failed: {message}")]
    Orbit {
        /// Stable adapter failure code.
        code: &'static str,
        /// Safe diagnostic without callback payloads or credentials.
        message: String,
    },
    /// A template file has invalid or incomplete rules.
    #[error("invalid template {file}, field {field}: {reason}")]
    TemplateInvalid {
        /// The caller-supplied template file name.
        file: String,
        /// The refused or missing rule field.
        field: String,
        /// Safe diagnostic, never the raw rule value.
        reason: &'static str,
    },
    /// A page does not use its template's fixed id or id pattern.
    #[error("page {id} does not match template {template}'s id rule {pattern}")]
    PageIdMismatch {
        /// Page id being checked.
        id: String,
        /// Template name.
        template: String,
        /// Expected fixed id or pattern.
        pattern: String,
    },
    /// A page is in the wrong knowledge layer for its template.
    #[error("page {id} has type {actual}; template {template} requires {expected}")]
    TemplateTypeMismatch {
        /// Page id being checked.
        id: String,
        /// Template name.
        template: String,
        /// Required page type.
        expected: &'static str,
        /// Supplied page type.
        actual: &'static str,
    },
    /// A required level-two section is absent or has no content.
    #[error("page {id} needs a nonempty ## {section} section for template {template}")]
    MissingSection {
        /// Page id being checked.
        id: String,
        /// Template name.
        template: String,
        /// Required heading text.
        section: String,
    },
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
    /// A model call failed; `retryable` follows the HTTP failure class.
    ///
    /// The message names only the endpoint host, status and operation:
    /// never a secret, request body or response body (STD-05).
    #[error("model operation ({code}) failed: {message}")]
    Model {
        /// Stable adapter error code.
        code: &'static str,
        /// Safe diagnostic, without secrets or model payloads.
        message: String,
        /// Whether retrying the same request could succeed.
        retryable: bool,
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
            Self::OrbitUnavailable => "orbit_unavailable",
            Self::OrbitOutcomeUnknown => "orbit_outcome_unknown",
            Self::OrbitRefused { .. } => "orbit_refused",
            Self::Orbit { code, .. } => code,
            Self::TemplateInvalid { .. } => "template_invalid",
            Self::PageIdMismatch { .. } => "page_id_mismatch",
            Self::TemplateTypeMismatch { .. } => "template_type_mismatch",
            Self::MissingSection { .. } => "missing_section",
            Self::LibraryUnavailable => "library_unavailable",
            Self::Library { code, .. } => code,
            Self::Model { code, .. } => code,
            Self::InvalidSetting { .. } => "invalid_library_setting",
            Self::InvalidRequest(_) => "invalid_request",
            Self::UnsupportedVersion(_) => "unsupported_schema_version",
            Self::UnknownTool(_) => "unknown_tool",
            Self::InvalidArgument(_) => "invalid_argument",
        }
    }

    /// Whether retrying the same request could succeed. Protocol refusals
    /// are deterministic; only a model failure carries its own verdict
    /// (a rate-limit or server failure may succeed on retry).
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::Model { retryable, .. } => *retryable,
            Self::OrbitUnavailable => false,
            Self::OrbitOutcomeUnknown => false,
            Self::OrbitRefused { .. } => false,
            Self::Orbit { .. } => false,
            Self::TemplateInvalid { .. } => false,
            Self::PageIdMismatch { .. } => false,
            Self::TemplateTypeMismatch { .. } => false,
            Self::MissingSection { .. } => false,
            Self::LibraryUnavailable => false,
            Self::Library { .. } => false,
            Self::InvalidSetting { .. } => false,
            Self::InvalidRequest(_) => false,
            Self::UnsupportedVersion(_) => false,
            Self::UnknownTool(_) => false,
            Self::InvalidArgument(_) => false,
        }
    }
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
