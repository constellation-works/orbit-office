//! `office.signals.scan`: Run the proposal signals and return candidate
//! proposals with evidence.
//!
//! Planned: see `docs/design/office/specs/proposals.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "signals.scan",
    execution_kind: ExecutionKind::ReadOnly,
    mcp_scope: McpScope::Workspace,
});
