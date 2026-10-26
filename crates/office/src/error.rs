//! The CLI crate's surface error type (STD-02 R11, R12): core failures are
//! translated once, through the single `#[from]` below.

/// Everything the surface can fail with after a successful parse.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// A core protocol failure that escaped envelope handling.
    #[error(transparent)]
    Core(#[from] office_core::Error),
    /// Writing the response envelope failed (other than a closed pipe,
    /// which the output module treats as success; STD-01 R13).
    #[error("cannot write the response: {0}")]
    Output(#[source] std::io::Error),
}

impl Error {
    /// A stable `snake_case` code for machine output (STD-01 R19).
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Core(error) => error.code(),
            Self::Output(_) => "output_error",
        }
    }
}

/// `Result` with this crate's [`Error`].
pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;
