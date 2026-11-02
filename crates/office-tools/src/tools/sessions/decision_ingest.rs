//! `office.decision-ingest`: append answered Orbit human-decision requests
//! to the decision ledgers (specs/decision-precedents.md, "Ingesting Orbit
//! decisions" and "Feedback").
//!
//! Model-free. Reads every request in the followed workspaces (the Office
//! workspace, then each `brief.task_workspaces` selector) through the
//! `orbit.decision.list` callback with `include_closed`, falls back to
//! `orbit.decision.show` for an answered request listed without its
//! resolution, and reads each new entry's task type through
//! `orbit.task.show`. The pure planner maps, links and skips; one commit
//! writes every ledger it changed. `dry_run` reports the same plan and
//! writes nothing. A followed extra workspace that cannot be read is
//! reported in `unavailable`; the Office workspace failing refuses the run.

use office_core::domain::source::decision_ingest::{self, Listed};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "decision-ingest",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["since", "dry_run"],
    run,
);

fn invalid(message: &str) -> Error {
    Error::InvalidArgument(format!("decision-ingest {message}"))
}

fn object(value: Value) -> Result<Map<String, Value>> {
    match value {
        Value::Object(map) => Ok(map),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Err(invalid("callback input"))
        }
    }
}

fn with_workspace(mut input: Map<String, Value>, selector: Option<&str>) -> Map<String, Value> {
    if let Some(selector) = selector {
        input.insert("workspace".into(), json!(selector));
    }
    input
}

/// One workspace's requests, with each answered request's resolution
/// filled from `orbit.decision.show` when the listing omitted it.
fn read_workspace(call: &Call<'_>, selector: Option<&str>) -> Result<Vec<Listed>> {
    let input = with_workspace(object(json!({"include_closed": true}))?, selector);
    let response = call.ports.orbit.decision_list(&input)?;
    let requests = response
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("orbit.decision.list returned no requests list"))?;
    let mut out = Vec::new();
    for request in requests {
        let mut request = request.clone();
        let answered = request.get("state").and_then(Value::as_str) == Some("answered");
        let missing = request.get("resolution").is_none_or(Value::is_null);
        if answered
            && missing
            && let Some(id) = request.get("id").and_then(Value::as_str)
        {
            let input = with_workspace(object(json!({"id": id}))?, selector);
            if let Ok(shown) = call.ports.orbit.decision_show(&input)
                && shown.get("id").and_then(Value::as_str) == Some(id)
                && let Some(resolution) = shown.get("resolution").filter(|value| !value.is_null())
                && let Some(map) = request.as_object_mut()
            {
                map.insert("resolution".into(), resolution.clone());
            }
        }
        out.push(Listed {
            selector: selector.map(str::to_owned),
            request,
        });
    }
    Ok(out)
}

/// The Orbit task's type, or `None` when it cannot be read.
fn task_type(call: &Call<'_>, task: &str, selector: Option<&str>) -> Option<String> {
    let input = with_workspace(
        object(json!({"id": task, "fields": ["id", "type"]})).ok()?,
        selector,
    );
    let shown = call.ports.orbit.task_show(&input).ok()?;
    (shown.get("id").and_then(Value::as_str) == Some(task))
        .then(|| shown.get("type").and_then(Value::as_str).map(str::to_owned))
        .flatten()
}

fn run(call: &Call<'_>) -> Result<Value> {
    let dry_run = match call.input.get("dry_run") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(invalid("dry_run must be a boolean")),
    };
    let since = match call.input.get("since") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.as_str()),
        Some(_) => return Err(invalid("since must be an RFC 3339 time")),
    };
    let library = &call.ports.library;
    let settings = library.settings()?;
    let ledgers: Vec<(String, String)> = library
        .decision_ledgers()?
        .into_iter()
        .map(|file| (file.path, file.markdown.unwrap_or_default()))
        .collect();
    let mut listed = read_workspace(call, None)?;
    let mut unavailable = Vec::new();
    for selector in &settings.brief.task_workspaces {
        match read_workspace(call, Some(selector)) {
            Ok(requests) => listed.extend(requests),
            Err(_) => unavailable.push(selector.clone()),
        }
    }
    let now = format_rfc3339_utc(call.ports.clock.now());
    let plan = decision_ingest::plan(&ledgers, &listed, since, &now, |task, selector| {
        task_type(call, task, selector)
    })?;
    if !dry_run {
        library.write_decision_ledgers(&plan.files)?;
    }
    Ok(json!({
        "dry_run": dry_run,
        "since": since,
        "appended": plan.appended,
        "skipped": plan.skipped,
        "dedupe": plan.dedupe,
        "unavailable": unavailable,
    }))
}
