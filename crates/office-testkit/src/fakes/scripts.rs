//! Recording script runner; no filesystem or subprocess access.
use office_core::{
    Result,
    domain::script::{
        ScriptBounds, ScriptError, ScriptExecution, ScriptFile, ScriptPreparation, ScriptResult,
    },
    ports::ScriptRunner,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

type ExecutionReply = Result<(ScriptResult, Vec<ScriptFile>)>;

/// Shared observer for script preparation/execution calls and queued results.
#[derive(Clone, Default)]
pub struct FakeScripts {
    /// Exact preparation requests.
    pub preparations: Rc<RefCell<Vec<ScriptPreparation>>>,
    /// Exact executions.
    pub executions: Rc<RefCell<Vec<ScriptExecution>>>,
    replies: Rc<RefCell<VecDeque<ExecutionReply>>>,
}
impl FakeScripts {
    /// Queue an explicit execution outcome.
    pub fn push_response(&self, reply: Result<(ScriptResult, Vec<ScriptFile>)>) {
        self.replies.borrow_mut().push_back(reply);
    }
}
impl ScriptRunner for FakeScripts {
    fn prepare(&self, script: &ScriptPreparation) -> Result<()> {
        self.preparations.borrow_mut().push(script.clone());
        Ok(())
    }
    fn run(
        &self,
        execution: &ScriptExecution,
        _bounds: &ScriptBounds,
    ) -> Result<(ScriptResult, Vec<ScriptFile>)> {
        self.executions.borrow_mut().push(execution.clone());
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(ScriptError::Unavailable.into()))
    }
}
