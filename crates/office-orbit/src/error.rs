//! Typed callback failures; raw child output never crosses this boundary.

/// Failures of Orbit's callback channel.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A name outside Office's manifest allowlist was refused before spawning.
    #[error("callback tool is not in Office's orbit_tools allowlist")]
    ToolNotAllowed,
    /// Orbit returned a structured refusal on stderr.
    #[error("Orbit refused the callback ({code})")]
    Refused {
        /// Machine-readable code; raw error messages and details are discarded.
        code: String,
    },
    /// A successful subprocess returned an invalid JSON document.
    #[error("Orbit callback returned invalid JSON")]
    InvalidResponse,
    /// A subprocess failed without a structured Orbit refusal.
    #[error("Orbit callback exited unsuccessfully (exit code {exit_code:?})")]
    CommandFailed {
        /// None means termination by signal.
        exit_code: Option<i32>,
    },
    /// The bounded process supervisor failed or terminated the call.
    #[error("Orbit callback subprocess failed")]
    Process(#[from] office_common::Error),
    /// A mutating callback started but its reply could not be verified.
    #[error("Orbit callback outcome is unknown; inspect Orbit before retrying")]
    OutcomeUnknown(#[source] Box<Error>),
}

impl Error {
    /// Stable Office failure code; Orbit's own refusal code is in [`Self::Refused`].
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ToolNotAllowed => "orbit_tool_not_allowed",
            Self::Refused { .. } => "orbit_refused",
            Self::InvalidResponse => "orbit_invalid_response",
            Self::CommandFailed { .. } => "orbit_command_failed",
            Self::Process(_) => "orbit_transport_failed",
            Self::OutcomeUnknown(_) => "orbit_outcome_unknown",
        }
    }
}

/// The one translation to the pure tool-facing error boundary.
impl From<Error> for office_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Refused { code } => Self::OrbitRefused { code },
            Error::OutcomeUnknown(_) => Self::OrbitOutcomeUnknown,
            other @ (Error::ToolNotAllowed
            | Error::InvalidResponse
            | Error::CommandFailed { .. }
            | Error::Process(_)) => Self::Orbit {
                code: other.code(),
                message: other.to_string(),
            },
        }
    }
}

/// Result of a callback operation.
pub type Result<T> = std::result::Result<T, Error>;
