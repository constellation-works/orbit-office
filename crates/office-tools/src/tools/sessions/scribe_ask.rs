//! `office.scribe-ask`: a cited, model-backed answer over the session SCR
//! files (specs/scribe.md, "Answering").
//!
//! Integrator scaffold only: the tool is not built yet. The feature task
//! switches `TOOL` to `Tool::built`, adds the handler and its schemas,
//! and uncomments the reserved block in `plugin.yaml`.

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "scribe-ask",
    execution_kind: ExecutionKind::ReadOnly,
    mcp_scope: McpScope::Workspace,
});
