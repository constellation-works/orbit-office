//! Every tool this backend knows, one row each, built or planned.
//!
//! Integrator-owned (ARCHITECTURE.md): the scaffold wrote a row for every
//! planned tool, so a task that builds one edits only that tool's own
//! module. A planned tool has no handler; dispatch refuses it as
//! `unknown_tool` and the manifest does not list it (STD-01 R36).

use office_core::Result;
use office_core::tool::ToolSpec;
use serde_json::{Map, Value};

use crate::tools::{
    artifacts, bootstrap, insights, knowledge, library, proposals, scripts, selection, sessions,
    sources, status,
};

/// What a handler receives: the tool's validated input and Orbit's call context.
pub(crate) struct Call<'a> {
    /// Tool input, already checked against the tool's accepted keys.
    pub(crate) input: &'a Map<String, Value>,
    /// Services available to this call.
    pub(crate) ports: &'a office_core::ports::Ports<'a>,
    /// Call context: identity, secrets and roots, from Orbit.
    pub(crate) context: &'a Map<String, Value>,
}

/// A tool handler: input and context in, the output payload out.
pub(crate) type Handler = fn(&Call<'_>) -> Result<Value>;

/// One registry row.
#[derive(Clone, Copy)]
pub struct Tool {
    spec: ToolSpec,
    accepts: &'static [&'static str],
    handler: Option<Handler>,
}

impl Tool {
    /// A tool that is specified but not built: no handler, not advertised.
    pub(crate) const fn planned(spec: ToolSpec) -> Self {
        Self {
            spec,
            accepts: &[],
            handler: None,
        }
    }

    /// A built tool: the input keys it accepts and its handler.
    pub(crate) const fn built(
        spec: ToolSpec,
        accepts: &'static [&'static str],
        handler: Handler,
    ) -> Self {
        Self {
            accepts,
            handler: Some(handler),
            ..Self::planned(spec)
        }
    }

    /// The tool's contract: name, execution kind, MCP scope.
    #[must_use]
    pub fn spec(&self) -> ToolSpec {
        self.spec
    }

    /// The input keys a built tool accepts (empty for a planned tool).
    #[must_use]
    pub fn accepts(&self) -> &'static [&'static str] {
        self.accepts
    }

    /// Whether the tool is built (has a handler) and so is served and advertised.
    #[must_use]
    pub fn is_built(&self) -> bool {
        self.handler.is_some()
    }

    pub(crate) fn handler(&self) -> Option<Handler> {
        self.handler
    }
}

/// Every tool, in manifest order.
static TOOLS: &[Tool] = &[
    status::TOOL,
    // sources (specs/library.md)
    sources::capture::TOOL,
    sources::transcribe::TOOL,
    sources::video_plan::TOOL,
    sources::frames::TOOL,
    sources::extract::TOOL,
    sources::show::TOOL,
    sources::set_sensitivity::TOOL,
    // knowledge (specs/library.md)
    knowledge::list::TOOL,
    knowledge::show::TOOL,
    knowledge::write::TOOL,
    // artifacts (specs/library.md)
    artifacts::list::TOOL,
    artifacts::show::TOOL,
    artifacts::promote::TOOL,
    // library (specs/library.md, specs/plugin-surface.md)
    library::init::TOOL,
    library::reindex::TOOL,
    library::config_set::TOOL,
    library::private_route_check::TOOL,
    library::search::TOOL,
    library::attach::TOOL,
    // proposals (specs/proposals.md)
    proposals::signals_scan::TOOL,
    proposals::create::TOOL,
    proposals::accept::TOOL,
    // selection (specs/selection-sharing.md)
    selection::get::TOOL,
    selection::put::TOOL,
    // sessions (specs/sessions.md)
    sessions::brief::TOOL,
    sessions::digest::TOOL,
    // scripts (specs/scripts.md)
    scripts::run::TOOL,
    // bootstrap (specs/library-bootstrap.md)
    bootstrap::batch_plan::TOOL,
    bootstrap::consolidate::TOOL,
    bootstrap::status::TOOL,
    // insights (specs/analytics-and-reviews.md)
    insights::metrics::TOOL,
    insights::review_inputs::TOOL,
];

/// Every tool this backend knows, built or planned, in manifest order.
#[must_use]
pub fn tools() -> &'static [Tool] {
    TOOLS
}
