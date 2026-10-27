//! `office.init`: Scaffold the Library: layout, git repository, config,
//! .gitignore, index; idempotent.
//!
//! The handler only talks to the Library port; scaffolding, git and the
//! index live in the adapter (specs/library.md, "Scaffolding: `office.init`").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "init",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["library_dir"],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let library_dir = match call.input.get("library_dir") {
        None => None,
        Some(Value::String(dir)) => Some(dir.as_str()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "init library_dir must be a string".to_string(),
            ));
        }
    };
    let outcome = call.ports.library.initialize(library_dir)?;
    Ok(json!({
        "already_initialized": outcome.already_initialized,
        "created": outcome.created,
    }))
}
