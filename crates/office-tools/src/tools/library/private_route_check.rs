//! `office.private-route-check`: check the configured open-weights endpoint
//! with a tiny text, image and audio request and record which it accepts.
//!
//! Planned: see `docs/design/office/specs/models-and-privacy.md`. To build
//! it, switch `TOOL` to `Tool::built` with its accepted keys and handler,
//! add its schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "private-route-check",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
