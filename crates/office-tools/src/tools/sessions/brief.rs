//! Read-only session continuity, assembled without inference or writes.

use std::collections::BTreeMap;

use office_core::domain::session::{agent_runs, assemble, invalid, tasks};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "session-brief",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let library = &call.ports.library;
    let head = library.head_commit()?;
    let sources = library.source_documents()?;
    let artifacts = library.artifact_documents()?;
    let pages = library.knowledge_pages()?;
    let templates = library.templates()?;
    let input = json!({"limit":10000,"fields":["id","title","status","tags","created_at","updated_at","history","job_run_id"]});
    let value = call.ports.orbit.task_list(
        input
            .as_object()
            .ok_or_else(|| invalid("task-list request"))?,
    )?;
    let rows = tasks(&value)?;
    let mut deliveries = BTreeMap::new();
    for (id, run_id) in agent_runs(rows)? {
        let input = json!({"id":id,"field":"delivery","run_id":run_id});
        let delivery = call.ports.orbit.task_show(
            input
                .as_object()
                .ok_or_else(|| invalid("delivery request"))?,
        )?;
        if delivery.get("task_id").and_then(Value::as_str) != Some(id.as_str())
            || delivery.get("run_id").and_then(Value::as_str) != Some(run_id.as_str())
        {
            return Err(invalid("delivery identity"));
        }
        deliveries.insert(id, delivery);
    }
    let brief = assemble(&sources, rows, &artifacts, &pages, &templates, &deliveries)?;
    if library.head_commit()? != head {
        return Err(Error::Library {
            code: "library_changed",
            message: "Library changed while reading the session brief; retry".into(),
        });
    }
    serde_json::to_value(brief).map_err(|_| invalid("brief response"))
}
