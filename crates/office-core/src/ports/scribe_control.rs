//! Scribe control port. The adapter owns `plugin_state/scribe_control.sqlite`.
//!
//! Tools pass the call's clock in. The record rules live in
//! [`crate::domain::scribe_control`] so a fake and the SQLite adapter
//! cannot drift.

use std::time::SystemTime;

use crate::domain::scribe_control::{Change, Control};
use crate::{Error, Result};

/// The per-session control operations used by `office.scribe-control`.
pub trait ScribeControls {
    /// Read one session's record; a session never written reads as the default.
    fn get(&self, session_id: &str) -> Result<Control>;

    /// Apply one change atomically and return the new record and whether a
    /// pending Capture-now request was consumed.
    fn apply(&self, session_id: &str, change: &Change, now: SystemTime) -> Result<(Control, bool)>;
}

/// A control port for contexts with no plugin state. Every call refuses.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableScribeControls;

impl ScribeControls for UnavailableScribeControls {
    fn get(&self, _session_id: &str) -> Result<Control> {
        Err(unavailable())
    }

    fn apply(
        &self,
        _session_id: &str,
        _change: &Change,
        _now: SystemTime,
    ) -> Result<(Control, bool)> {
        Err(unavailable())
    }
}

fn unavailable() -> Error {
    Error::Library {
        code: "scribe_control_unavailable",
        message: "scribe control state is unavailable in this call context".to_string(),
    }
}
