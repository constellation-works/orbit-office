//! `office.signals-scan`: Run the proposal signals and return candidate
//! proposals with evidence.
//!
//! Reads tasks and frictions through the Orbit port and thresholds plus
//! Library pages/sources through the Library port, then runs the pure
//! signals domain (specs/proposals.md#signals). Writes nothing. A failed
//! read or a truncated task list is refused by name; it is not reported
//! as an empty or partial candidate set.

use std::collections::BTreeMap;
use std::path::Path;

use office_core::domain::knowledge::{DocumentRecord, LinkKind, Page};
use office_core::domain::signals::{
    FrictionFact, PageFact, SignalFacts, SourceFact, TaskFact, scan,
};
use office_core::domain::source::parse_source_participants;
use office_core::ports::LibraryText;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "signals-scan",
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
    // Fail closed (STD-02 R29, R31): these reads are the scan's integrity,
    // not a side channel. A failed read or a truncated task page is named
    // and refused, never turned into an empty or partial candidate count.
    let tasks = call
        .ports
        .orbit
        .task_list(&task_input)
        .map_err(|error| refuse_read("orbit.task.list", "failed", &error))?;
    if tasks.get("truncated").and_then(Value::as_bool) == Some(true) {
        return Err(refuse_truncated(&tasks));
    }
    facts.tasks = parse_tasks(&tasks, now_days);

    // Frictions through Orbit search (listing mode: no query).
    let mut friction_input = Map::new();
    friction_input.insert("kind".into(), json!("friction"));
    friction_input.insert("all".into(), json!(true));
    let frictions = call
        .ports
        .orbit
        .search(&friction_input)
        .map_err(|error| refuse_read("orbit.search", "friction search failed", &error))?;
    facts.frictions = parse_frictions(&frictions);

    load_library_facts(call, now_days, &mut facts)?;

    let candidates = scan(&facts, &settings.thresholds);
    Ok(json!({
        "candidates": candidates,
        "count": candidates.len(),
    }))
}

/// Enumeration returns complete Markdown, independent of search's limits and
/// excerpts. These reads are integrity checks: refuse an unreadable document
/// or backlink lookup rather than presenting a partial scan as complete.
fn load_library_facts(call: &Call<'_>, now_days: i64, facts: &mut SignalFacts) -> Result<()> {
    let sources = call
        .ports
        .library
        .source_documents()
        .map_err(|error| refuse_read("library.source_documents", "failed", &error))?;
    let mut source_dates = BTreeMap::new();
    for file in sources {
        let markdown = markdown(&file, "library.source_documents")?;
        let record = DocumentRecord::from_markdown(Path::new(&file.path), markdown);
        let participants = parse_source_participants(markdown)
            .map_err(|error| refuse_read("library.source_documents", &file.path, &error))?;
        source_dates.insert(
            record.id.clone(),
            record.date.as_deref().and_then(date_days),
        );
        facts.sources.push(SourceFact {
            id: record.id,
            participants,
            tags: record.tags,
            body: record.body,
        });
    }
    let pages = call
        .ports
        .library
        .knowledge_pages()
        .map_err(|error| refuse_read("library.knowledge_pages", "failed", &error))?;
    for file in pages {
        let page = Page::from_markdown(markdown(&file, "library.knowledge_pages")?)
            .map_err(|error| refuse_read("library.knowledge_pages", &file.path, &error))?;
        let cites = page
            .citations()
            .map_err(|error| refuse_read("library.knowledge_pages", &file.path, &error))?
            .into_iter()
            .map(|citation| citation.id)
            .collect();
        let verified = page.last_verified.as_deref().and_then(date_days);
        let backlinks = call
            .ports
            .library
            .backlinks(&page.id)
            .map_err(|error| refuse_read("library.backlinks", &page.id, &error))?;
        let mut cited_by: Vec<String> = backlinks
            .into_iter()
            .filter(|link| link.via.contains(&LinkKind::Cites))
            .filter(|link| {
                verified
                    .zip(source_dates.get(&link.id).copied().flatten())
                    .is_some_and(|(verified, source)| source > verified)
            })
            .map(|link| link.id)
            .collect();
        cited_by.sort();
        cited_by.dedup();
        facts.pages.push(PageFact {
            id: page.id,
            title: page.title,
            aliases: page.aliases,
            tags: page.tags,
            last_verified_days_ago: verified
                .map(|date| u32::try_from((now_days - date).max(0)).unwrap_or(u32::MAX)),
            // This is a mechanical review candidate; a proposal still needs
            // quoted evidence of the contradiction or extension (spec).
            has_newer_evidence: !cited_by.is_empty(),
            cited_by,
            cites,
        });
    }
    Ok(())
}

fn markdown<'a>(file: &'a LibraryText, read: &'static str) -> Result<&'a str> {
    file.markdown
        .as_deref()
        .ok_or_else(|| Error::SignalsScanRefused {
            read,
            code: "library_document_unreadable",
            detail: format!("cannot read {}", file.path),
        })
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

/// Name the failed read. The envelope code stays the underlying class, and
/// the original message is kept so a refusal code is not dropped.
fn refuse_read(read: &'static str, failure: &str, error: &Error) -> Error {
    let code = error.code();
    Error::SignalsScanRefused {
        read,
        detail: format!("{failure} ({code}): {error}"),
        code,
    }
}

/// A task page that reports truncation is not the complete task set.
fn refuse_truncated(value: &Value) -> Error {
    let detail = match value.get("total").and_then(Value::as_u64) {
        Some(total) => format!(
            "returned a truncated page (total {total}); the scan does not treat that page as the \
             complete task set"
        ),
        None => {
            "returned a truncated page; the scan does not treat that page as the complete task set"
                .to_string()
        }
    };
    Error::SignalsScanRefused {
        read: "orbit.task.list",
        detail,
        code: "task_list_truncated",
    }
}
