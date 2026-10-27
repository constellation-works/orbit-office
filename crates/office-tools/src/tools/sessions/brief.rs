//! `office.session-brief`: assemble the session-start brief from Orbit and
//! the Library — waiting on you, agents working on, open questions, the
//! last session's decisions — with no model call.
//!
//! Planned: see `docs/design/office/specs/sessions.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "session-brief",
    execution_kind: ExecutionKind::ReadOnly,
    mcp_scope: McpScope::Workspace,
});
