//! Session records (SCR files) that earlier builds' scribe wrote into the
//! Library's `sources/conversation/<yyyy>/<mm>/`. The scribe moved to the
//! standalone scribe plugin (4_decisions.md); this build writes no SCR and
//! keeps only what reads the existing files: their path rules and the v2
//! reader the session brief uses. Pure: callers read through the Library
//! port.

mod paths;
pub mod v2;

pub use paths::{is_scr_path, validate_scr_path};

use crate::Error;

/// Longest title slug in an SCR file name.
pub const SLUG_MAX: usize = 48;
/// Session-id characters appended to a clashing SCR name.
pub const CLASH_SUFFIX: usize = 6;
/// Library directory of the SCR files.
pub const SCR_DIR: &str = "sources/conversation";

fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}
