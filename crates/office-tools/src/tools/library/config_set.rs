//! `office.config-set`: Validate and write one Library setting in
//! .office/config.yaml; commit.
//!
//! The Library port applies the typed update to the latest settings while
//! holding its writer lock, so concurrent changes to different keys survive.

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "config-set",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["key", "value"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let key = call
        .input
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::InvalidArgument("config-set key must be a string".into()))?;
    let value = call.input.get("value").ok_or_else(|| {
        Error::InvalidArgument(
            "config-set requires value (null is allowed for retention, open_weights and demo.simulated_until)".into(),
        )
    })?;
    call.ports.library.set_setting(key, value.clone())?;
    Ok(json!({"path": ".office/config.yaml", "key": key}))
}
