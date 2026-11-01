//! In-memory scribe control records. The rules are the domain's, so tests
//! and the SQLite adapter agree.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use office_core::domain::scribe_control::{Change, Control};
use office_core::ports::ScribeControls;
use office_core::{Error, Result};

/// Scribe control records kept in the process. Clones share the records.
#[derive(Clone, Debug, Default)]
pub struct FakeScribeControls {
    records: Arc<Mutex<BTreeMap<String, Control>>>,
}

impl ScribeControls for FakeScribeControls {
    fn get(&self, session_id: &str) -> Result<Control> {
        Ok(self.lock()?.get(session_id).cloned().unwrap_or_default())
    }

    fn apply(&self, session_id: &str, change: &Change, now: SystemTime) -> Result<(Control, bool)> {
        let mut records = self.lock()?;
        records.retain(|_, control| !control.expired(now));
        let control = records.entry(session_id.to_owned()).or_default();
        let consumed = control.apply(change, now)?;
        Ok((control.clone(), consumed))
    }
}

impl FakeScribeControls {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, Control>>> {
        self.records.lock().map_err(|_| Error::Library {
            code: "scribe_control_unavailable",
            message: "scribe control records lock was poisoned".to_string(),
        })
    }
}
