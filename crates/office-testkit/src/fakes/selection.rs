//! In-memory selection tray. The rules are the domain tray, so tests and
//! the SQLite adapter agree.

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use office_core::domain::selection::{PutRequest, PutResult, Selection, Tray};
use office_core::ports::Selections;
use office_core::{Error, Result};

/// A selection tray that keeps its rows in the process.
#[derive(Clone, Debug, Default)]
pub struct FakeSelections {
    tray: Arc<Mutex<Tray>>,
}

impl Selections for FakeSelections {
    fn put(&self, request: &PutRequest, now: SystemTime) -> Result<PutResult> {
        Ok(self.lock()?.put(request, now))
    }

    fn peek(&self, now: SystemTime) -> Result<Vec<Selection>> {
        Ok(self.lock()?.peek(now))
    }

    fn get(&self, now: SystemTime) -> Result<Vec<Selection>> {
        Ok(self.lock()?.get(now))
    }
}

impl FakeSelections {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Tray>> {
        self.tray.lock().map_err(|_| Error::Library {
            code: "selection_unavailable",
            message: "selection tray lock was poisoned".to_string(),
        })
    }
}
