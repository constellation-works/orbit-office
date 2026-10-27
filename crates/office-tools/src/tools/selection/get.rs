//! `office.selection-get`: Return the user's fresh, unused selections from
//! the app and mark them consumed.
//!
//! Planned: see `docs/design/office/specs/selection-sharing.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "selection-get",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::Workspace,
});
