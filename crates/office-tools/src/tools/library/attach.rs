//! `office.attach`: Pick the knowledge, source and example artifact files
//! for a task and copy them into it as task artifacts under context/.
//!
//! Planned: see `docs/design/office/specs/library.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "attach",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::Workspace,
});
