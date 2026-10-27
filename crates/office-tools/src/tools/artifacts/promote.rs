//! `office.artifact-promote`: Copy an approved task's output into the
//! Library as an artifact; write provenance; commit.
//!
//! Planned: see `docs/design/office/specs/library.md`. To build it, switch
//! `TOOL` to `Tool::built` with its accepted keys and handler, add its
//! schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "artifact-promote",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
