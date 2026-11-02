//! Host-facing, read-only `orbit.precedent-provider/1`. No model or callback.
use office_core::Result;
use office_core::domain::source::decision_precedents::{self, Request};
use office_core::ports::Sensitivity;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::Value;

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "decision-precedents",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::None,
    },
    &["contract", "workspace_id", "request", "as_of", "k"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let request = Request::parse(Value::Object(call.input.clone()))?;
    let library = &call.ports.library;
    let settings = library.settings()?;
    let files = library.decision_precedent_ledgers()?;
    let private = ["decisions", "conversation"]
        .iter()
        .any(|kind| settings.sensitivity.get(*kind) == Some(&Sensitivity::Private));
    let pool = decision_precedents::Pool::read(&request, &files, private);
    let scores = if pool.documents.is_empty() {
        Default::default()
    } else {
        library.decision_precedent_scores(&request.lexical_query(), &pool.documents)?
    };
    decision_precedents::response(
        &request,
        &pool,
        &scores,
        settings.scribe.precedents.half_life_days,
    )
}
