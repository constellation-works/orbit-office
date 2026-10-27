//! In-memory calendar port, including failure preservation and offline files.
use office_core::domain::calendar::{CalendarError, Snapshot};
use office_core::{Result, ports::Calendar};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Clones share snapshot, failure and offline ICS fixtures.
#[derive(Clone, Default)]
pub struct FakeCalendar {
    state: Rc<RefCell<State>>,
}
#[derive(Default)]
struct State {
    snapshot: Snapshot,
    error: Option<String>,
    files: BTreeMap<String, String>,
}
impl FakeCalendar {
    /// Stage a workspace-relative UTF-8 ICS fixture.
    pub fn file(&self, path: &str, text: &str) {
        self.state
            .borrow_mut()
            .files
            .insert(path.into(), text.into());
    }
}
impl Calendar for FakeCalendar {
    fn load(&self) -> Result<(Snapshot, Option<String>)> {
        let state = self.state.borrow();
        Ok((state.snapshot.clone(), state.error.clone()))
    }
    fn replace(&self, snapshot: &Snapshot) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.snapshot = snapshot.clone();
        state.error = None;
        Ok(())
    }
    fn record_error(&self, code: &str) -> Result<()> {
        self.state.borrow_mut().error = Some(code.into());
        Ok(())
    }
    fn read_ics(&self, path: &str) -> Result<String> {
        self.state
            .borrow()
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| CalendarError::Invalid.into())
    }
}
