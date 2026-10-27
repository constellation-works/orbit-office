//! `office.status`: the plugin version, which declared secrets are set
//! (never a secret value; STD-05), and Library and index health.
//! Reading health creates no Library state.

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
    // An unavailable Library adapter (no roots in context) reads as
    // uninitialized health; status itself never refuses for that.
    let health = call
        .ports
        .library
        .health()
        .unwrap_or_else(|_| office_core::ports::LibraryHealth::uninitialized());
    Ok(json!({
        "version": VERSION,
        "secrets": secrets,
        "library": {
            "initialized": health.initialized,
            "documents": health.documents_total,
            "documents_by_kind": {
                "source": health.source,
                "domain": health.domain,
                "runbook": health.runbook,
                "artifact": health.artifact,
            },
        },
    }))
}
