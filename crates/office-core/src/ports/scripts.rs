//! Script dependency preparation and offline execution.
use crate::domain::script::{
    ScriptBounds, ScriptExecution, ScriptFile, ScriptPreparation, ScriptResult,
};
use crate::{Result, domain::script::ScriptError};

/// A configured adapter owns all script filesystem and child process access.
pub trait ScriptRunner {
    /// Resolve inline PEP 723 dependencies once, when accepting a proposal.
    fn prepare(&self, _script: &ScriptPreparation) -> Result<()> {
        Err(ScriptError::Unavailable.into())
    }
    /// Verify hash and all physical input paths before creating any run directory.
    fn run(
        &self,
        _execution: &ScriptExecution,
        _bounds: &ScriptBounds,
    ) -> Result<(ScriptResult, Vec<ScriptFile>)> {
        Err(ScriptError::Unavailable.into())
    }
}
