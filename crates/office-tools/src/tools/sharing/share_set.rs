//! Explicit sharing approval, hidden from MCP.
use crate::registry::{Call, Tool};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "share-set",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["id", "shareable"],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Error::InvalidArgument("id must be a nonempty string".into()))?;
    let shareable = call
        .input
        .get("shareable")
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::InvalidArgument("shareable must be true or false".into()))?;
    let record = call.ports.library.set_shareable(id, shareable)?;
    Ok(json!({"id":record.id,"path":record.path,"shareable":record.shareable}))
}
