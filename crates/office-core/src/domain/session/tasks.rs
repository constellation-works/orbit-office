//! Orbit task inventory validation and task sections.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{Error, Result};

use super::helpers::{invalid, text, time_key};
use super::item::{Item, Section};

/// Validate a complete Orbit projection, including pagination metadata.
pub fn tasks(value: &Value) -> Result<&[Value]> {
    let rows = value
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("orbit.task.list.tasks"))?;
    match value.get("truncated").and_then(Value::as_bool) {
        Some(false) => {}
        Some(true) => return Err(Error::Library {
            code: "task_list_truncated",
            message:
                "orbit.task.list is truncated; session brief requires the complete task inventory"
                    .into(),
        }),
        None => return Err(invalid("orbit.task.list.truncated")),
    }
    if value.get("total").and_then(Value::as_u64) != u64::try_from(rows.len()).ok() {
        return Err(invalid("orbit.task.list.total"));
    }
    let mut ids = BTreeSet::new();
    for row in rows {
        if !ids.insert(text(row, "id")?) {
            return Err(invalid("unique task ids"));
        }
        tags(row)?;
    }
    Ok(rows)
}

pub(super) fn tags(row: &Value) -> Result<Vec<&str>> {
    row.get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("task.tags"))?
        .iter()
        .map(|t| t.as_str().ok_or_else(|| invalid("task.tags")))
        .collect()
}

/// Runs whose typed delivery evidence is needed to distinguish failure from a
/// blocked task or a later metadata edit. No failure is inferred from prose.
pub fn agent_runs(rows: &[Value]) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for row in rows {
        let tags = tags(row)?;
        if tags.contains(&"owner:agent")
            && !tags.contains(&"proposal")
            && task_active(text(row, "status")?)?
        {
            match row.get("job_run_id") {
                Some(Value::String(id)) if !id.is_empty() => {
                    result.push((text(row, "id")?.into(), id.clone()))
                }
                None | Some(Value::Null) => {}
                Some(_) => return Err(invalid("task.job_run_id")),
            }
        }
    }
    Ok(result)
}

/// Whether an Orbit task status is open (not `done`, `rejected` or
/// `archived`); an unknown status is not open.
#[must_use]
pub fn task_open(status: &str) -> bool {
    task_active(status).unwrap_or(false)
}

pub(super) fn task_active(status: &str) -> Result<bool> {
    match status {
        "proposed" | "backlog" | "in-progress" | "review" | "blocked" | "someday" => Ok(true),
        "done" | "rejected" | "archived" => Ok(false),
        _ => Err(invalid("task.status")),
    }
}

fn task_item(row: &Value, at: &str, status: &str, workspace: Option<&str>) -> Result<Item> {
    let id = text(row, "id")?;
    Ok(Item {
        id: id.into(),
        link: format!("orbit://task/{id}"),
        text: text(row, "title")?.into(),
        time: Some(at.into()),
        entry_kind: None,
        status: Some(status.into()),
        template: None,
        workspace: workspace.map(str::to_string),
        corrected: None,
        by: None,
        structured_handoff: None,
        legacy: None,
        listing: None,
    })
}

/// One workspace's validated task inventory. `workspace: None` is the office
/// workspace; `Some(selector)` is an extra workspace from
/// `brief.task_workspaces`, read through `orbit.task.list` with that selector.
pub struct TaskInventory<'a> {
    /// Extra workspace selector, or `None` for the office workspace.
    pub workspace: Option<&'a str>,
    /// Validated rows from that workspace's `orbit.task.list`.
    pub rows: &'a [Value],
}

pub(crate) fn task_sections(
    inventories: &[TaskInventory<'_>],
    since: Option<&str>,
    deliveries: &BTreeMap<String, Value>,
) -> Result<(Section, Section)> {
    let cutoff = since.map(time_key).transpose()?;
    let mut waiting = Vec::new();
    let mut working = Vec::new();
    let mut seen = BTreeSet::new();
    for inventory in inventories {
        let external = inventory.workspace.is_some();
        for row in inventory.rows {
            if !seen.insert(text(row, "id")?.to_string()) {
                continue;
            }
            let item = classify_task(
                row,
                inventory.workspace,
                external,
                &cutoff,
                deliveries,
                &mut waiting,
                &mut working,
            );
            // An extra workspace's malformed row must not refuse the whole
            // brief; the handler reports such a workspace as unavailable
            // before assembly, so a residual error here is skipped only for
            // external rows. Office-workspace errors stay all-or-nothing.
            match item {
                Ok(()) => {}
                Err(error) if external => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
    waiting.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.id.cmp(&y.id)));
    working.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.id.cmp(&y.id)));
    Ok((
        Section::new(
            "waiting_on_you",
            waiting.into_iter().map(|(_, i)| i).collect(),
        ),
        Section::new(
            "agents_working_on",
            working.into_iter().map(|(_, i)| i).collect(),
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
fn classify_task(
    row: &Value,
    workspace: Option<&str>,
    external: bool,
    cutoff: &Option<(i64, String)>,
    deliveries: &BTreeMap<String, Value>,
    waiting: &mut Vec<((i64, String), Item)>,
    working: &mut Vec<((i64, String), Item)>,
) -> Result<()> {
    {
        let tags = tags(row)?;
        let agent = tags.contains(&"owner:agent");
        let you = tags.contains(&"owner:you");
        let proposal = tags.contains(&"proposal");
        // Office workspace: only office-tagged work, exactly as before.
        // Extra workspaces do not use office ownership tags, so there the
        // documented rule is broader: any `review` task waits on the user,
        // and any non-proposal in-progress/finished/failed task is working.
        if !external && !agent && !you && !proposal {
            return Ok(());
        }
        let status = text(row, "status")?;
        let active = task_active(status)?;
        let created = text(row, "created_at")?;
        time_key(created)?;
        let waits = (proposal && status == "proposed")
            || (!proposal
                && ((agent && status == "review")
                    || (you && active)
                    || (external && status == "review")));
        if waits {
            waiting.push((
                time_key(created)?,
                task_item(row, created, status, workspace)?,
            ));
        }
        if proposal || (!external && !agent) {
            return Ok(());
        }
        let mut at = None;
        let mut working_status = status;
        if status == "in-progress" {
            at = Some(text(row, "updated_at")?.to_string());
        }
        if status == "done" {
            let history = row
                .get("history")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("task.history"))?;
            // Completion is the latest transition to done, never updated_at.
            let mut finished = history
                .iter()
                .filter(|e| e.get("to_status").and_then(Value::as_str) == Some("done"))
                .map(|e| {
                    let at = text(e, "at")?;
                    Ok((time_key(at)?, at.to_string()))
                })
                .collect::<Result<Vec<_>>>()?;
            finished.sort();
            let (key, stamp) = finished
                .pop()
                .ok_or_else(|| invalid("task completion history"))?;
            if cutoff.as_ref().is_none_or(|since| key > *since) {
                at = Some(stamp);
            }
        }
        if let Some(delivery) = deliveries.get(text(row, "id")?) {
            let state = text(delivery, "run_state")?;
            if active && state == "failed" {
                // A failed run is no longer working, even if task promotion has
                // not yet changed its in-progress lifecycle state.
                at = None;
                let stamp = text(delivery, "run_finished_at")?;
                let key = time_key(stamp)?;
                if cutoff.as_ref().is_none_or(|since| key > *since) {
                    at = Some(stamp.into());
                    working_status = "failed";
                }
            }
        }
        if let Some(at) = at {
            working.push((
                time_key(&at)?,
                task_item(row, &at, working_status, workspace)?,
            ));
        }
        Ok(())
    }
}

/// Runs needing delivery evidence in an extra workspace, where office
/// ownership tags do not apply: any active non-proposal task with a run.
pub fn agent_runs_external(rows: &[Value]) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for row in rows {
        let tags = tags(row)?;
        if tags.contains(&"proposal") || !task_active(text(row, "status")?)? {
            continue;
        }
        match row.get("job_run_id") {
            Some(Value::String(id)) if !id.is_empty() => {
                result.push((text(row, "id")?.into(), id.clone()))
            }
            None | Some(Value::Null) => {}
            Some(_) => return Err(invalid("task.job_run_id")),
        }
    }
    Ok(result)
}
