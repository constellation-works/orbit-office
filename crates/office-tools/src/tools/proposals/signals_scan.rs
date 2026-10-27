//! `office.signals.scan`: Run the proposal signals and return candidate
//! proposals with evidence.
//!
//! Reads tasks and frictions through the Orbit port and thresholds plus
//! Library pages/sources through the Library port, then runs the pure
//! signals domain (specs/proposals.md#signals). Writes nothing.

use office_core::Result;
use office_core::domain::knowledge::SearchFilter;
use office_core::domain::signals::{
    FrictionFact, PageFact, SignalFacts, SourceFact, TaskFact, scan,
};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "signals.scan",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    // Thresholds come from Library settings on every call.
    let settings = call.ports.library.settings()?;
    let now_days = call
        .ports
        .clock
        .now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400).cast_signed())
        .unwrap_or(0);

    let mut facts = SignalFacts::default();

    // Tasks through the Orbit port.
    let mut task_input = Map::new();
    task_input.insert("limit".into(), json!(200));
    task_input.insert(
        "fields".into(),
        json!([
            "id",
            "title",
            "status",
            "description",
            "tags",
            "created_at",
            "updated_at"
        ]),
    );
    if let Ok(value) = call.ports.orbit.task_list(&task_input) {
        facts.tasks = parse_tasks(&value, now_days);
    }

    // Frictions through Orbit search (listing mode: no query).
    let mut friction_input = Map::new();
    friction_input.insert("kind".into(), json!("friction"));
    friction_input.insert("all".into(), json!(true));
    if let Ok(value) = call.ports.orbit.search(&friction_input) {
        facts.frictions = parse_frictions(&value);
    }

    // Library pages and sources through the Library port.
    if let Ok(hits) = call.ports.library.search(
        "",
        &SearchFilter {
            limit: Some(1000),
            ..SearchFilter::default()
        },
    ) {
        for hit in hits {
            match hit.kind {
                office_core::domain::knowledge::DocumentKind::Source => {
                    facts.sources.push(SourceFact {
                        id: hit.id,
                        tags: Vec::new(),
                        participants: Vec::new(),
                        body: hit.snippet,
                    });
                }
                office_core::domain::knowledge::DocumentKind::Domain
                | office_core::domain::knowledge::DocumentKind::Runbook => {
                    facts.pages.push(PageFact {
                        id: hit.id,
                        title: hit.title,
                        ..PageFact::default()
                    });
                }
                office_core::domain::knowledge::DocumentKind::Artifact => {}
            }
        }
    }

    let candidates = scan(&facts, &settings.thresholds);
    Ok(json!({
        "candidates": candidates,
        "count": candidates.len(),
    }))
}

/// Records array under any of the envelope keys Orbit uses.
fn records(value: &Value) -> Vec<&Value> {
    if let Some(array) = value.as_array() {
        return array.iter().collect();
    }
    for key in ["tasks", "hits", "results", "items", "records"] {
        if let Some(array) = value.get(key).and_then(Value::as_array) {
            return array.iter().collect();
        }
    }
    Vec::new()
}

/// Unwrap a hit wrapper (`record`/`task`) to the record itself.
fn unwrap(value: &Value) -> &Value {
    value
        .get("record")
        .or_else(|| value.get("task"))
        .unwrap_or(value)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// Days from a `YYYY-MM-DD` prefix (Howard Hinnant's civil algorithm).
fn date_days(text: &str) -> Option<i64> {
    let mut parts = text.split(['-', 'T', ' ']);
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn days_ago(task: &Value, now_days: i64) -> Option<u32> {
    if let Some(days) = task.get("completed_days_ago").and_then(Value::as_u64) {
        return u32::try_from(days).ok();
    }
    for key in ["completed_at", "updated_at", "created_at"] {
        if let Some(days) = task
            .get(key)
            .and_then(Value::as_str)
            .and_then(date_days)
            .map(|d| now_days - d)
            .and_then(|d| u32::try_from(d).ok())
        {
            return Some(days);
        }
    }
    None
}

fn parse_tasks(value: &Value, now_days: i64) -> Vec<TaskFact> {
    records(value)
        .iter()
        .map(|item| {
            let task = unwrap(item);
            let tags = strings(task.get("tags"));
            TaskFact {
                id: text(task, &["id"]),
                title: text(task, &["title"]),
                description: text(task, &["description"]),
                status: text(task, &["status"]),
                completed_days_ago: days_ago(task, now_days),
                user_corrected: task
                    .get("user_corrected")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| tags.iter().any(|t| t == "corrected")),
                has_runbook: task
                    .get("has_runbook")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| tags.iter().any(|t| t == "has_runbook")),
                tags,
            }
        })
        .filter(|t| !t.id.is_empty())
        .collect()
}

fn parse_frictions(value: &Value) -> Vec<FrictionFact> {
    records(value)
        .iter()
        .map(|item| {
            let friction = unwrap(item);
            FrictionFact {
                id: text(friction, &["id"]),
                text: text(friction, &["text", "title", "description", "summary"]),
                link: ["link", "linked_id", "same_as"]
                    .iter()
                    .find_map(|key| friction.get(*key).and_then(Value::as_str))
                    .map(str::to_string),
            }
        })
        .filter(|f| !f.id.is_empty())
        .collect()
}
