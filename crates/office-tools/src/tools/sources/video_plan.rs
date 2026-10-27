//! `office.video.plan`: Plan a video source's processing: duration,
//! deduplicated keyframes, model-call counts and a cost estimate; with
//! `confirm` and the digest the user saw, approve it for the KT routine.
//!
//! Planned: see `docs/design/office/specs/kt-videos.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "video.plan",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
