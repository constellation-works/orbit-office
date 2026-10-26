//! Tool contract types. Every tool module in `office-tools` declares one
//! [`ToolSpec`]; the manifest (`plugin.yaml`) is held equal to them by
//! `crates/office/tests/plugin_tree.rs`.

/// Whether a tool changes state (the manifest's `execution_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionKind {
    /// Writes nothing.
    ReadOnly,
    /// Writes the Library, plugin state or Orbit records.
    Mutating,
}

impl ExecutionKind {
    /// The manifest token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Mutating => "mutating",
        }
    }
}

/// Where a tool is visible over MCP (the manifest's `mcp_scope`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpScope {
    /// Visible to agents in the workspace.
    Workspace,
    /// Hidden from MCP: only `orbit tool run` (the app, the install script) may call it.
    None,
}

impl McpScope {
    /// The manifest token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::None => "none",
        }
    }
}

/// What a tool is, independent of whether it is built yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    /// The verb after `office.`, for example `capture` or `knowledge.write`.
    pub name: &'static str,
    /// Whether it changes state.
    pub execution_kind: ExecutionKind,
    /// Its MCP visibility.
    pub mcp_scope: McpScope,
}
