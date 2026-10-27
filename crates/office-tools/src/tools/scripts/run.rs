//! `office.script.run`: run an approved, pinned `script` artifact in its
//! own run directory under plugin state, offline, within its bounds, and
//! file its outputs as a `script-output` task for review; run only by
//! routine and auto-task steps.
//!
//! Planned: see `docs/design/office/specs/scripts.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "script.run",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
