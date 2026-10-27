//! `office.digest`: write the day's digest artifact (series `digest-daily`)
//! with one Muse Spark call through the privacy routing; run only by the
//! `office-daily-digest` routine.
//!
//! Planned: see `docs/design/office/specs/sessions.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "digest",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
