//! Read-only session continuity, assembled without inference or writes.

use std::collections::BTreeMap;

use office_core::domain::session::{
    TaskInventory, agent_runs, agent_runs_external, assemble_with_workspaces, invalid, tasks,
};
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
    let library = &call.ports.library;
    let head = library.head_commit()?;
    let sources = library.source_documents()?;
    let artifacts = library.artifact_documents()?;
    let pages = library.knowledge_pages()?;
    let templates = library.templates()?;
    let settings = library.settings()?;
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
    let brief = assemble_with_workspaces(
        &sources,
        &inventories,
        &unavailable,
        &artifacts,
        &pages,
        &templates,
        &deliveries,
    )?;
    if library.head_commit()? != head {
        return Err(Error::Library {
            code: "library_changed",
            message: "Library changed while reading the session brief; retry".into(),
        });
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
