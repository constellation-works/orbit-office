//! Orbit callbacks port. Orbit owns callback input validation and authorization.

use serde_json::{Map, Value};

use crate::{Error, Result};

/// Exactly the task, artifact, friction, search and auto-task callbacks used by
/// Office.
///
/// Inputs are forwarded intact to Orbit's versioned tool schemas, including
/// unknown keys so Orbit can refuse them. Identity and credentials are supplied
/// by the host callback session, never added to these inputs by the adapter.
/// Defaults refuse access when no callback adapter is configured.
pub trait Orbit {
    /// Create a task.
    fn task_add(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Update a task.
    fn task_update(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// List tasks, retaining Orbit's totals and truncation metadata.
    fn task_list(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Read a task or its requested projection.
    fn task_show(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Read a task artifact.
    fn task_artifact_get(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Attach a file as a task artifact.
    fn task_artifact_put(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// File a friction record.
    fn friction_add(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Search Orbit records.
    fn search(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Create an auto-task definition.
    fn auto_task_add(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// Update an auto-task definition, including enabling or disabling it.
    fn auto_task_update(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
    /// List auto-task definitions.
    fn auto_task_list(&self, _input: &Map<String, Value>) -> Result<Value> {
        Err(Error::OrbitUnavailable)
    }
}
