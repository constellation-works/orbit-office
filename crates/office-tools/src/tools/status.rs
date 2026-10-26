//! `office.status`: the plugin version and which declared secrets are set;
//! never a secret value (STD-05: secrets never cross the tool boundary out).
//!
//! Filesystem and index health join this output with the `reindex` slice,
//! once the Library layout exists to check.

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Result, SECRET_NAMES, VERSION};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "status",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let present: Vec<&str> = call
        .context
        .get("secrets")
        .and_then(Value::as_object)
        .map(|secrets| secrets.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let secrets: Map<String, Value> = SECRET_NAMES
        .iter()
        .map(|name| {
            let state = if present.contains(name) {
                "set"
            } else {
                "unset"
            };
            ((*name).to_string(), json!(state))
        })
        .collect();
    Ok(json!({
        "version": VERSION,
        "secrets": secrets,
    }))
}
