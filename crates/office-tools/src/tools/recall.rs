//! `office.recall`: read-only selection of captured evidence and live refs.
use office_core::domain::source::conversation::recall::{self, Query, Reference};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "recall",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["question", "since", "until", "project", "limit"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let query: Query = serde_json::from_value(Value::Object(call.input.clone())).map_err(|_| {
        Error::InvalidArgument("recall requires question and valid optional filters".into())
    })?;
    let head = call.ports.library.head_commit()?;
    let matches = recall::select(&call.ports.library.source_documents()?, &query)?;
    let refs: Vec<_> = matches.refs.iter().map(|r| live_ref(call, r)).collect();
    if call.ports.library.head_commit()? != head {
        return Err(Error::Library {
            code: "library_changed",
            message: "Library changed while reading recall; retry".into(),
        });
    }
    Ok(
        json!({"entries":matches.entries,"sessions":matches.sessions,"refs":refs,"coverage":matches.coverage}),
    )
}

fn unresolved(reference: &Reference, reason: &str) -> Value {
    json!({"kind":reference.kind,"id":reference.id,"status":"unresolved","reason":reason})
}
fn live_ref(call: &Call<'_>, reference: &Reference) -> Value {
    match reference.kind.as_str() {
        "task" => {
            let input = json!({"id":reference.id});
            let Some(input) = input.as_object() else {
                return unresolved(reference, "task_unavailable");
            };
            match call.ports.orbit.task_show(input) {
                Ok(task) => {
                    if task["id"].as_str() != Some(&reference.id) {
                        return unresolved(reference, "invalid_task_response");
                    }
                    let Some(status) = task["status"].as_str().filter(|s| {
                        matches!(
                            *s,
                            "proposed"
                                | "backlog"
                                | "in-progress"
                                | "review"
                                | "done"
                                | "blocked"
                                | "someday"
                                | "rejected"
                                | "archived"
                        )
                    }) else {
                        return unresolved(reference, "invalid_task_response");
                    };
                    json!({"kind":"task","id":reference.id,"status":status,
                        "workspace":task.get("workspace").or_else(|| task.get("workspace_id")).or_else(|| task.get("workspace_path")).cloned().unwrap_or(Value::Null)})
                }
                // Adapter errors may contain private paths or upstream text. Return
                // a fixed reason, never echo the callback error to a recall caller.
                Err(_) => unresolved(reference, "task_unavailable"),
            }
        }
        "pr" => unresolved(reference, "no_pr_source"),
        "commit" => unresolved(reference, "no_commit_source"),
        _ => unresolved(reference, "unsupported_ref"),
    }
}
