//! `office.selection-get`: Return the user's fresh, unused selections from
//! the app and mark them consumed.

use office_core::Result;
use office_core::domain::selection::to_json;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "selection-get",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let selections = call.ports.selections.get(call.ports.clock.now())?;
    Ok(json!({
        "selections": selections.iter().map(to_json).collect::<Vec<_>>(),
    }))
}
