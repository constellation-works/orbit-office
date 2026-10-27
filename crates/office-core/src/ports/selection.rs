//! Selection tray port. The adapter owns `plugin_state/selections.sqlite`.
//!
//! Tools pass the call's clock in. The tray rules themselves live in
//! [`crate::domain::selection`] so a fake and the SQLite adapter cannot drift.

use std::time::SystemTime;

use crate::domain::selection::{PutRequest, PutResult, Selection};
use crate::{Error, Result};

/// The selection operations used by `office.selection-put` and
/// `office.selection-get`.
pub trait Selections {
    /// Record a selection, optionally emptying the tray first.
    fn put(&self, request: &PutRequest, now: SystemTime) -> Result<PutResult>;

    /// Return every fresh selection and mark each one consumed.
    fn get(&self, now: SystemTime) -> Result<Vec<Selection>>;
}

/// A selection port for contexts that have no tray. Every call refuses.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableSelections;

impl Selections for UnavailableSelections {
    fn put(&self, _request: &PutRequest, _now: SystemTime) -> Result<PutResult> {
        Err(unavailable())
    }

    fn get(&self, _now: SystemTime) -> Result<Vec<Selection>> {
        Err(unavailable())
    }
}

fn unavailable() -> Error {
    Error::Library {
        code: "selection_unavailable",
        message: "selection storage is unavailable in this call context".to_string(),
    }
}
