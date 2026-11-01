//! `office.scribe-control`: Office-owned per-session scribe control state (planned).
//!
//! Integrator scaffold only: the tool is not built yet. The feature task
//! switches `TOOL` to `Tool::built`, adds the handler and its schemas,
//! and uncomments the reserved block in `plugin.yaml`.

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "scribe-control",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::Workspace,
});
