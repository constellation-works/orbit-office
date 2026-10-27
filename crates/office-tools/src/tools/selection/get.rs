//! `office.selection-get`: Return the user's fresh, unused selections from
//! the app, optionally leaving them unconsumed.

use office_core::domain::selection::to_json;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "selection-get",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["peek"],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let peek = match call.input.get("peek") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "selection-get peek must be a boolean".into(),
            ));
        }
    };
    let now = call.ports.clock.now();
    let selections = if peek {
        call.ports.selections.peek(now)?
    } else {
        call.ports.selections.get(now)?
    };
    Ok(json!({
        "selections": selections.iter().map(to_json).collect::<Vec<_>>(),
    }))
}
