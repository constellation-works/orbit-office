//! `office.batch-plan`: plan a bootstrap phase's batch (calls and cost)
//! and confirm it with the digest the user saw.
//!
//! Planned: see `docs/design/office/specs/library-bootstrap.md`. To build
//! it, switch `TOOL` to `Tool::built` with its accepted keys and handler,
//! add its schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "batch-plan",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
