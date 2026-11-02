//! `office.decision-resolve`: settle one open decision conflict in one call.
//! Records which entry stands as a new ledger entry that `reverses` the
//! other, closes the conflict to-do, and releases the agent to-dos it held
//! (specs/scribe.md, "Meeting decisions"). Model-free.
use office_core::domain::source::decision_precedents::{OPEN_TODO, conflicting_pair};
use office_core::domain::source::decisions::{DecidedBy, views};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};
use crate::tools::sources::extract::filing::{ACTS_ON, HELD_TAG, HELD_UNTIL};
use crate::tools::sources::extract::ledger::read_ledgers;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "decision-resolve",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["task", "stands"],
    run,
);

fn refusal(code: &'static str, message: String) -> Error {
    Error::Library { code, message }
}

fn text<'a>(call: &'a Call<'_>, key: &str) -> Result<&'a str> {
    call.input
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::InvalidArgument(format!("decision-resolve {key} is required")))
}

fn object(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or(Error::OrbitUnavailable)
}

fn run(call: &Call<'_>) -> Result<Value> {
    let task = text(call, "task")?;
    let stands = text(call, "stands")?;
    let shown = call.ports.orbit.task_show(object(
        &json!({"id":task,"fields":["id","status","tags","description"]}),
    )?)?;
    let record = shown.get("task").unwrap_or(&shown);
    let not_conflict = || {
        refusal(
            "decision_conflict_not_found",
            format!("{task} is not an open decision-conflict to-do"),
        )
    };
    if record["id"].as_str() != Some(task) {
        return Err(not_conflict());
    }
    let (left, right) = conflicting_pair(record).ok_or_else(not_conflict)?;
    let other = if stands == left {
        right
    } else if stands == right {
        left
    } else {
        return Err(Error::InvalidArgument(format!(
            "decision-resolve stands must be {left} or {right}"
        )));
    };
    let ledgers = read_ledgers(call)?;
    let entries = views(&ledgers);
    let find = |id: &str| {
        entries.iter().find(|e| e.decision.id == id).ok_or_else(|| {
            refusal(
                "decision_not_found",
                format!("no decision ledger holds {id}"),
            )
        })
    };
    let standing = find(stands)?;
    let overturned = find(&other)?;
    // Retry-safe: a conflict already settled is closed, not recorded twice.
    let recorded = if let Some(by) = &overturned.reversed_by {
        by.clone()
    } else {
        if let Some(by) = &standing.reversed_by {
            return Err(refusal(
                "decision_reversed",
                format!(
                    "{stands} was already reversed by {by}; resolve against the current decision"
                ),
            ));
        }
        let mut decision = standing.decision.clone();
        let scope = decision.parsed_scope()?;
        decision.id = scope.entry_id(1);
        decision.decided_by = DecidedBy::Human;
        decision.decided_at = format_rfc3339_utc(call.ports.clock.now());
        decision.reverses = Some(other.clone());
        decision.orbit = None;
        decision.same_as = None;
        decision.confirms = None;
        decision.validate()?;
        let outcome = call
            .ports
            .library
            .append_decision(&decision, &decision.decided_at)?;
        outcome.entry.decision.id
    };
    let (released, withdrawn) = release(call, task, stands, &other)?;
    call.ports.orbit.task_update(object(&json!({"id":task,"status":"done",
        "comment":format!("Resolved: {stands} stands, recorded as {recorded}, which reverses {other}.")}))?)?;
    Ok(
        json!({"task":task,"stands":stands,"reverses":other,"entry":recorded,"released":released,"withdrawn":withdrawn}),
    )
}

/// The open held to-dos this conflict holds: one acting on the standing
/// entry is filed for agents in `meetings.agent_workspace` and closed; one
/// acting on the overturned entry is rejected.
fn release(
    call: &Call<'_>,
    task: &str,
    stands: &str,
    other: &str,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let workspace = call.ports.library.settings()?.meetings.agent_workspace;
    let listed = call.ports.orbit.task_list(object(
        &json!({"tag":["owner:you",HELD_TAG],"status":OPEN_TODO,"limit":1000,
        "fields":["id","title","type","tags","description","acceptance_criteria"]}),
    )?)?;
    let until = format!("{HELD_UNTIL}{task} is resolved.");
    let mut released = Vec::new();
    let mut withdrawn = Vec::new();
    for held in listed["tasks"].as_array().into_iter().flatten() {
        let (Some(id), Some(description)) = (held["id"].as_str(), held["description"].as_str())
        else {
            continue;
        };
        let lines: Vec<&str> = description.lines().map(str::trim).collect();
        if !lines.contains(&until.as_str()) {
            continue;
        }
        let acts_on = lines.iter().find_map(|line| line.strip_prefix(ACTS_ON));
        if acts_on == Some(stands)
            && let Some(workspace) = &workspace
        {
            let mut tags = vec![json!("owner:agent")];
            tags.extend(
                held["tags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|t| t.as_str().is_some_and(|t| t.starts_with("source:")))
                    .cloned(),
            );
            let body = description
                .split("\n\nHeld from agents:")
                .next()
                .unwrap_or(description);
            let filed = call.ports.orbit.task_add(object(&json!({"workspace":workspace,"title":held["title"],
                "description":format!("{body}\n\nReleased when {task} settled that {stands} stands."),
                "type":held["type"],"complexity":"medium","origin":"automation","tags":tags,
                "acceptance_criteria":held["acceptance_criteria"]}))?)?;
            let filed_id = filed["id"].clone();
            call.ports.orbit.task_update(object(&json!({"id":id,"status":"done",
                "comment":format!("Filed for agents as {} after {task} settled that {stands} stands.", filed_id.as_str().unwrap_or("a new task"))}))?)?;
            released.push(json!({"held":id,"task":filed_id}));
        } else if acts_on == Some(other) {
            call.ports.orbit.task_update(object(&json!({"id":id,"status":"rejected",
                "comment":format!("Not filed for agents: {task} settled that {stands} stands over {other}, the decision this acts on.")}))?)?;
            withdrawn.push(json!(id));
        }
    }
    Ok((released, withdrawn))
}
