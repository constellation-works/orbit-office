//! `office.frames`: Extract a confirmed video's planned keyframes and read
//! each with Muse Spark image input into `frames.md`; run only by the KT
//! routine.
//!
//! Planned: see `docs/design/office/specs/kt-videos.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "frames",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
