//! Recording Orbit port with explicit queued replies and no host I/O.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use office_core::ports::Orbit;
use office_core::{Error, Result};
use serde_json::{Map, Value};

/// A call made through one of the eleven Orbit port methods.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCall {
    /// Canonical callback tool name.
    pub tool: &'static str,
    /// Full input, including nested artifact or task data.
    pub input: Map<String, Value>,
}

#[derive(Default)]
struct State {
    calls: Vec<OrbitCall>,
    responses: VecDeque<Result<Value>>,
}

/// Clones share calls and queued responses, so tests can keep an observer
/// after handing the fake to boxed ports. Unconfigured replies fail explicitly.
#[derive(Clone, Default)]
pub struct FakeOrbit {
    state: Rc<RefCell<State>>,
}

impl FakeOrbit {
    /// Queue the next reply (success or typed failure), in call order.
    pub fn push_response(&self, response: Result<Value>) {
        self.state.borrow_mut().responses.push_back(response);
    }

    /// Snapshot the calls, including calls that returned errors.
    #[must_use]
    pub fn calls(&self) -> Vec<OrbitCall> {
        self.state.borrow().calls.clone()
    }

    fn record(&self, tool: &'static str, input: &Map<String, Value>) -> Result<Value> {
        let mut state = self.state.borrow_mut();
        state.calls.push(OrbitCall {
            tool,
            input: input.clone(),
        });
        state
            .responses
            .pop_front()
            .unwrap_or(Err(Error::OrbitUnavailable))
    }
}

impl Orbit for FakeOrbit {
    fn task_add(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.add", input)
    }
    fn task_update(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.update", input)
    }
    fn task_list(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.list", input)
    }
    fn task_show(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.show", input)
    }
    fn task_artifact_get(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.artifact.get", input)
    }
    fn task_artifact_put(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.task.artifact.put", input)
    }
    fn friction_add(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.friction.add", input)
    }
    fn search(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.search", input)
    }
    fn auto_task_add(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.auto_task.add", input)
    }
    fn auto_task_update(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.auto_task.update", input)
    }
    fn auto_task_list(&self, input: &Map<String, Value>) -> Result<Value> {
        self.record("orbit.auto_task.list", input)
    }
}
