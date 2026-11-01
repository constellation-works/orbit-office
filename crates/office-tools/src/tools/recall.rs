//! `office.recall`: answers about past conversations from captured entries
//! (specs/recall.md). Planned: no handler yet, so dispatch refuses it as
//! `unknown_tool` (STD-01 R36).

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "recall",
    execution_kind: ExecutionKind::ReadOnly,
    mcp_scope: McpScope::Workspace,
});
