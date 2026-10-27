//! `office.metrics`: recorded work and approvals, by UTC calendar week.

use office_core::domain::artifact::Artifact;
use office_core::domain::metrics::{Task, Window, compute, invalid_data};
use office_core::ports::LibraryText;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "metrics",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["since", "until"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let window = window(call)?;
    let facts = load(call)?;
    serde_json::to_value(compute(&window, &facts.tasks, &facts.artifacts)?)
        .map_err(|_| invalid_data("report"))
}

pub(crate) fn window(call: &Call<'_>) -> Result<Window> {
    let date = |key| {
        call.input
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| Error::InvalidArgument(format!("{key} must be a YYYY-MM-DD date")))
    };
    Window::new(date("since")?, date("until")?)
}

pub(crate) struct Facts {
    pub(crate) tasks: Vec<Task>,
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) library_commit: Option<String>,
}

pub(crate) fn markdown(file: &LibraryText) -> Result<&str> {
    file.markdown.as_deref().ok_or_else(|| Error::Library {
        code: "library_document_unreadable",
        message: format!("cannot read {}", file.path),
    })
}

/// Fetch the complete projected corpus in one callback; never silently use
/// Orbit's compact/default page or accept truncation as an empty history.
pub(crate) fn load(call: &Call<'_>) -> Result<Facts> {
    let library_commit = call.ports.library.head_commit()?;
    let input = json!({"limit": 10000, "fields": ["id", "title", "status", "type", "tags", "created_at", "history", "artifacts"]});
    let value = call.ports.orbit.task_list(
        input
            .as_object()
            .ok_or_else(|| invalid_data("task-list request"))?,
    )?;
    let rows = value
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_data("orbit.task.list.tasks"))?;
    let truncated = value
        .get("truncated")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_data("orbit.task.list.truncated"))?;
    if truncated {
        return Err(Error::Library {
            code: "task_list_truncated",
            message: "orbit.task.list is truncated; analytics require the complete work history"
                .into(),
        });
    }
    if value.get("total").and_then(Value::as_u64) != u64::try_from(rows.len()).ok() {
        return Err(invalid_data("orbit.task.list.total"));
    }
    let mut tasks = rows
        .iter()
        .map(Task::from_record)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    if tasks.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(invalid_data("unique task ids"));
    }
    let mut artifacts = call
        .ports
        .library
        .artifact_documents()?
        .iter()
        .map(|file| Ok(Artifact::from_document(&file.path, markdown(file)?)?))
        .collect::<Result<Vec<_>>>()?;
    artifacts.sort_by(|a, b| a.id.cmp(&b.id));
    unchanged(call, &library_commit)?;
    Ok(Facts {
        tasks,
        artifacts,
        library_commit,
    })
}

pub(crate) fn unchanged(call: &Call<'_>, commit: &Option<String>) -> Result<()> {
    if &call.ports.library.head_commit()? != commit {
        return Err(Error::Library {
            code: "library_changed",
            message: "Library changed while analytics read it; retry for one consistent snapshot"
                .into(),
        });
    }
    Ok(())
}
