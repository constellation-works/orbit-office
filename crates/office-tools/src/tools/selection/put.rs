//! `office.selection-put`: Record a selection from the app; clear: true
//! empties the tray.
//!
//! Planned: see `docs/design/office/specs/selection-sharing.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "selection-put",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
