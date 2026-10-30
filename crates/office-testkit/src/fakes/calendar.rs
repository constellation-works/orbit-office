//! In-memory calendar port, including failure preservation and offline files.
use office_core::domain::calendar::{
    CalendarError, CalendarSources, LocalCalendar, LocalChange, Snapshot,
};
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
    local: LocalCalendar,
    next_id: u64,
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
    fn sources(&self) -> Result<CalendarSources> {
        Ok(self.state.borrow().local.sources.clone())
    }
    fn change_local(&self, change: &LocalChange) -> Result<Option<String>> {
        let mut state = self.state.borrow_mut();
        state.next_id += 1;
        let key = format!("local-{}", state.next_id);
        state.local.apply(change, key)
    }
    fn load(&self) -> Result<(Snapshot, Option<String>)> {
        let state = self.state.borrow();
        let mut snapshot = if state.local.sources.external {
            state.snapshot.clone()
        } else {
            Snapshot::default()
        };
        if state.local.sources.local {
            snapshot.events.extend(state.local.events.clone());
        }
        Ok((
            snapshot,
            if state.local.sources.external {
                state.error.clone()
            } else {
                None
            },
        ))
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
