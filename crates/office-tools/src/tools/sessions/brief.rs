//! Read-only session continuity, assembled without inference or writes.

use std::collections::BTreeMap;

use office_core::domain::session::listing::{
    HISTORY_PAGE_DEFAULT, HISTORY_PAGE_MAX, ListQuery, list,
};
use office_core::domain::session::{
    SessionContext, TaskInventory, agent_runs, agent_runs_external, assemble_for_session,
    assignment_project, handoffs, invalid, task_open, tasks,
};
use office_core::domain::settings::HandoffMode;
use office_core::domain::source::format_rfc3339_utc;
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
    &[
        "project",
        "orchestrator",
        "task",
        "session_start",
        "view",
        "thread",
        "include_closed",
        "offset",
        "limit",
    ],
    run,
);

/// An optional current-session field: absent or null is `None`, a nonempty
/// single-line string is kept, anything else is a usage error.
fn optional_context(call: &Call<'_>, key: &str) -> Result<Option<String>> {
    match call.input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if !value.trim().is_empty()
                && value.len() <= 256
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(Error::InvalidArgument(format!(
            "session-brief {key} must be a nonempty single-line string"
        ))),
    }
}

/// The caller's assignment task id: 1–64 ASCII letters, digits, `-` or `_`.
fn optional_task(call: &Call<'_>) -> Result<Option<String>> {
    match call.input.get("task") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
        {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(Error::InvalidArgument(
            "session-brief task must be an Orbit task id".into(),
        )),
    }
}

fn session_start(call: &Call<'_>) -> Result<bool> {
    match call.input.get("session_start") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(Error::InvalidArgument(
            "session-brief session_start must be a boolean".into(),
        )),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Brief,
    Handoffs,
    List,
}

/// `view`: `brief` (the default), `handoffs` (the `/handoff` command) or
/// `list` (the web app's handoff list).
fn view(call: &Call<'_>) -> Result<View> {
    match call.input.get("view") {
        None | Some(Value::Null) => Ok(View::Brief),
        Some(Value::String(value)) if value == "brief" => Ok(View::Brief),
        Some(Value::String(value)) if value == "handoffs" => Ok(View::Handoffs),
        Some(Value::String(value)) if value == "list" => Ok(View::List),
        Some(_) => Err(Error::InvalidArgument(
            "session-brief view must be brief, handoffs or list".into(),
        )),
    }
}

/// An optional non-negative integer no larger than `max`.
fn optional_count(call: &Call<'_>, key: &str, max: usize) -> Result<Option<usize>> {
    match call.input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= max)
            .map(Some)
            .ok_or_else(|| {
                Error::InvalidArgument(format!(
                    "session-brief {key} must be an integer from 0 to {max}"
                ))
            }),
    }
}

/// Largest `offset` the list view accepts.
const OFFSET_MAX: usize = 100_000;

/// The `view: list` request: filters narrow `history`; paging bounds it.
fn list_query(call: &Call<'_>, context: &SessionContext) -> Result<ListQuery> {
    let include_closed = match call.input.get("include_closed") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "session-brief include_closed must be a boolean".into(),
            ));
        }
    };
    let limit = optional_count(call, "limit", HISTORY_PAGE_MAX)?.unwrap_or(HISTORY_PAGE_DEFAULT);
    if limit == 0 {
        return Err(Error::InvalidArgument(format!(
            "session-brief limit must be an integer from 1 to {HISTORY_PAGE_MAX}"
        )));
    }
    Ok(ListQuery {
        project: context.project.clone(),
        orchestrator: context.orchestrator.clone(),
        thread: optional_context(call, "thread")?,
        include_closed,
        offset: optional_count(call, "offset", OFFSET_MAX)?.unwrap_or(0),
        limit,
        now: context.now.clone(),
    })
}

fn library_changed() -> Error {
    Error::Library {
        code: "library_changed",
        message: "Library changed while reading the session brief; retry".into(),
    }
}

/// Whether Orbit reports `task` as an open task. Any failure, mismatch or
/// unknown status reads as not open: the fallback never refuses the brief.
fn assignment_open(call: &Call<'_>, task: &str) -> bool {
    let input = json!({"id": task, "fields": ["id", "status"]});
    let Some(input) = input.as_object() else {
        return false;
    };
    match call.ports.orbit.task_show(input) {
        Ok(value) => {
            value.get("id").and_then(Value::as_str) == Some(task)
                && value
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(task_open)
        }
        Err(_) => false,
    }
}

fn task_list_input(workspace: Option<&str>) -> Result<serde_json::Map<String, Value>> {
    let mut input = json!({"limit":10000,"fields":["id","title","status","tags","created_at","updated_at","history","job_run_id"]});
    if let Some(selector) = workspace {
        input["workspace"] = json!(selector);
    }
    Ok(input
        .as_object()
        .ok_or_else(|| invalid("task-list request"))?
        .clone())
}

fn delivery(
    call: &Call<'_>,
    id: &str,
    run_id: &str,
    workspace: Option<&str>,
    deliveries: &mut BTreeMap<String, Value>,
) -> Result<()> {
    let mut input = json!({"id":id,"field":"delivery","run_id":run_id});
    if let Some(selector) = workspace {
        input["workspace"] = json!(selector);
    }
    let delivery = call.ports.orbit.task_show(
        input
            .as_object()
            .ok_or_else(|| invalid("delivery request"))?,
    )?;
    if delivery.get("task_id").and_then(Value::as_str) != Some(id)
        || delivery.get("run_id").and_then(Value::as_str) != Some(run_id)
    {
        return Err(invalid("delivery identity"));
    }
    deliveries.insert(id.into(), delivery);
    Ok(())
}

fn run(call: &Call<'_>) -> Result<Value> {
    let mut context = SessionContext {
        project: optional_context(call, "project")?,
        orchestrator: optional_context(call, "orchestrator")?,
        now: Some(format_rfc3339_utc(call.ports.clock.now())),
        manual: false,
    };
    let task = optional_task(call)?;
    let start = session_start(call)?;
    let view = view(call)?;
    if task.is_some() && !start {
        return Err(Error::InvalidArgument(
            "session-brief task is the SessionStart fallback and requires session_start".into(),
        ));
    }
    if view != View::Brief && start {
        return Err(Error::InvalidArgument(
            "session-brief view handoffs or list cannot be combined with session_start".into(),
        ));
    }
    let list_keys = ["thread", "include_closed", "offset", "limit"];
    if view != View::List && list_keys.iter().any(|key| call.input.contains_key(*key)) {
        return Err(Error::InvalidArgument(
            "session-brief thread, include_closed, offset and limit require view list".into(),
        ));
    }
    let query = match view {
        View::List => Some(list_query(call, &context)?),
        View::Brief | View::Handoffs => None,
    };
    let library = &call.ports.library;
    let head = library.head_commit()?;
    let sources = library.source_documents()?;
    if view != View::Brief {
        let brief = match &query {
            Some(query) => list(&sources, query)?,
            None => handoffs(&sources, &context)?,
        };
        if library.head_commit()? != head {
            return Err(library_changed());
        }
        return serde_json::to_value(brief).map_err(|_| invalid("brief response"));
    }
    let artifacts = library.artifact_documents()?;
    let pages = library.knowledge_pages()?;
    let templates = library.templates()?;
    let settings = library.settings()?;
    // `brief.handoff` governs only the SessionStart hook's brief; an explicit
    // request (and `/handoff`) always gets the project it names.
    if start {
        match settings.brief.handoff {
            HandoffMode::Manual => context.manual = true,
            HandoffMode::Auto => {
                if context.project.is_none()
                    && let Some(task) = task.as_deref()
                    && assignment_open(call, task)
                {
                    context.project = assignment_project(&sources, task)?;
                }
                context.manual = context.project.is_none();
            }
        }
    }
    // Office workspace: unchanged, all-or-nothing.
    let value = call.ports.orbit.task_list(&task_list_input(None)?)?;
    let rows = tasks(&value)?;
    let mut deliveries = BTreeMap::new();
    for (id, run_id) in agent_runs(rows)? {
        delivery(call, &id, &run_id, None, &mut deliveries)?;
    }
    // Extra workspaces: any failure reading one becomes a single
    // workspace_unavailable item; it never refuses the brief.
    let mut extras: Vec<(String, Value, Vec<Value>)> = Vec::new();
    let mut unavailable: Vec<String> = Vec::new();
    for selector in &settings.brief.task_workspaces {
        match read_extra(call, selector) {
            Ok((response, extra_deliveries)) => {
                let extra_rows = match tasks(&response) {
                    Ok(rows) => rows.to_vec(),
                    Err(_) => {
                        unavailable.push(selector.clone());
                        continue;
                    }
                };
                deliveries.extend(extra_deliveries);
                extras.push((selector.clone(), response, extra_rows));
            }
            Err(_) => unavailable.push(selector.clone()),
        }
    }
    let mut inventories = vec![TaskInventory {
        workspace: None,
        rows,
    }];
    for (selector, _, extra_rows) in &extras {
        inventories.push(TaskInventory {
            workspace: Some(selector.as_str()),
            rows: extra_rows,
        });
    }
    let brief = assemble_for_session(
        &sources,
        &inventories,
        &unavailable,
        &artifacts,
        &pages,
        &templates,
        &deliveries,
        &context,
    )?;
    if library.head_commit()? != head {
        return Err(library_changed());
    }
    serde_json::to_value(brief).map_err(|_| invalid("brief response"))
}

fn read_extra(call: &Call<'_>, selector: &str) -> Result<(Value, BTreeMap<String, Value>)> {
    let response = call
        .ports
        .orbit
        .task_list(&task_list_input(Some(selector))?)?;
    let rows = tasks(&response)?;
    let mut deliveries = BTreeMap::new();
    for (id, run_id) in agent_runs_external(rows)? {
        delivery(call, &id, &run_id, Some(selector), &mut deliveries)?;
    }
    Ok((response, deliveries))
}
