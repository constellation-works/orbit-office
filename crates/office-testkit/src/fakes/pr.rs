//! Recording PR source with explicit queued replies and no host I/O.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use office_core::ports::{PrError, PrView, PullRequests};

#[derive(Default)]
struct State {
    calls: Vec<String>,
    responses: VecDeque<Result<PrView, PrError>>,
}

/// Clones share calls and queued responses, so tests can keep an observer
/// after handing the fake to boxed ports. Unconfigured replies are
/// unavailable, mirroring a context without the `gh` grant.
#[derive(Clone, Default)]
pub struct FakePullRequests {
    state: Rc<RefCell<State>>,
}

impl FakePullRequests {
    /// Queue the next reply (success or typed failure), in call order.
    pub fn push_response(&self, response: Result<PrView, PrError>) {
        self.state.borrow_mut().responses.push_back(response);
    }

    /// Snapshot the requested PR ids, including failed calls.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.state.borrow().calls.clone()
    }
}

impl PullRequests for FakePullRequests {
    fn pr_view(&self, id: &str) -> Result<PrView, PrError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(id.to_string());
        state
            .responses
            .pop_front()
            .unwrap_or(Err(PrError::Unavailable))
    }
}
