//! `office.consolidate`: group a confirmed batch's digests by subject,
//! apply the page rules, make one careful call per passing group and file
//! one proposal per page.
//!
//! Planned: see `docs/design/office/specs/library-bootstrap.md`. To build
//! it, switch `TOOL` to `Tool::built` with its accepted keys and handler,
//! add its schemas and golden, and uncomment its block in `plugin.yaml`
//! (ARCHITECTURE.md, "Tools").

use office_core::tool::{ExecutionKind, McpScope, ToolSpec};

use crate::registry::Tool;

pub(crate) const TOOL: Tool = Tool::planned(ToolSpec {
    name: "consolidate",
    execution_kind: ExecutionKind::Mutating,
    mcp_scope: McpScope::None,
});
