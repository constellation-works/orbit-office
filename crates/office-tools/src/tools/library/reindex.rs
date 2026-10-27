//! `office.reindex`: Rebuild the SQLite index from the Library.
//!
//! Files are the truth; the adapter rescans them under the writer lock.
//! An uninitialized Library is refused by the shared dispatch check
//! (`library_not_initialized`, STD-02 R24) before this handler runs.

use office_core::Result;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "reindex",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let indexed = call.ports.library.reindex()?;
    Ok(json!({ "indexed_documents": indexed }))
}
