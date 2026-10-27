//! User-facing monotonic source sensitivity update, hidden from MCP.

use crate::registry::{Call, Tool};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "source-set-sensitivity",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["id", "sensitivity"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let required = |key| {
        call.input
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Error::InvalidArgument(format!("{key} must be a nonempty string")))
    };
    let id = required("id")?;
    let sensitivity = required("sensitivity")?;
    if !matches!(sensitivity, "normal" | "private") {
        return Err(Error::InvalidArgument(
            "sensitivity must be normal or private".into(),
        ));
    }
    let source = call.ports.library.set_source_sensitivity(id, sensitivity)?;
    Ok(json!({"id":source.id,"path":source.path,"sensitivity":source.sensitivity}))
}
