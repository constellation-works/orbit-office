//! `office.review-inputs`: cited period material, gathered without a model.

use std::path::Path;

use office_core::Result;
use office_core::domain::knowledge::DocumentRecord;
use office_core::domain::metrics::{Window, compute, invalid_data, metadata, proposal_decision};
use office_core::domain::source::{is_conversation_kind, source_body};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::{Value, json};

use super::metrics::{load, markdown, unchanged, window};
use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "review-inputs",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["since", "until"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let window = window(call)?;
    let facts = load(call)?;
    let metrics = compute(&window, &facts.tasks, &facts.artifacts)?;
    let completed_tasks: Vec<_> = facts.tasks.iter().filter_map(|task| task.completion(&window, &facts.artifacts).map(|event| json!({
        "id":task.id, "title":task.title, "tags":task.tags, "completed_at":event.timestamp, "outputs":task.outputs,
    }))).collect();
    let accepted_proposals: Vec<_> = facts.tasks.iter().filter_map(|task| proposal_decision(task, &window, "done").map(|event| json!({
        "id":task.id, "title":task.title, "tags":task.tags, "accepted_at":event.timestamp, "outputs":task.outputs,
    }))).collect();
    let mut outputs = Vec::new();
    let mut daily_digests = Vec::new();
    for artifact in &facts.artifacts {
        if window.contains(&artifact.approved.at)? {
            let item = json!({"artifact":artifact, "path":artifact.path()?});
            if artifact.series == "digest-daily" {
                daily_digests.push(item);
            } else {
                outputs.push(item);
            }
        }
    }
    let conversation_sources = conversations(call, &window)?;
    let knowledge_changes = knowledge(call, &window)?;
    unchanged(call, &facts.library_commit)?;
    Ok(json!({
        "since":window.since, "until":window.until, "completed_tasks":completed_tasks,
        "accepted_proposals":accepted_proposals, "metrics":metrics, "outputs":outputs,
        "conversation_sources":conversation_sources, "daily_digests":daily_digests,
        "knowledge_changes":knowledge_changes,
    }))
}

fn conversations(call: &Call<'_>, window: &Window) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for file in call.ports.library.source_documents()? {
        let text = markdown(&file)?;
        let fields = metadata(text)?;
        if fields.get("kind").and_then(Value::as_str) != Some("conversation") {
            continue;
        }
        let record = DocumentRecord::from_markdown(Path::new(&file.path), text);
        let mut entries = Vec::new();
        for (index, line) in source_body(text).lines().enumerate() {
            let Some(rest) = line.strip_prefix('[') else {
                continue;
            };
            let (stamp, entry) = rest
                .split_once("] ")
                .ok_or_else(|| invalid_data("conversation entry"))?;
            let (kind, content) = entry
                .split_once(": ")
                .ok_or_else(|| invalid_data("conversation entry"))?;
            if !is_conversation_kind(kind) {
                return Err(invalid_data("conversation entry kind"));
            }
            if window.contains(stamp)? {
                entries.push(json!({"at":stamp,"kind":kind,"text":content,"cite":format!("{}#L{}",record.id,index + 1)}));
            }
        }
        if !entries.is_empty() {
            result.push(json!({"id":record.id,"title":record.title,"path":file.path,"sensitivity":record.sensitivity,"entries":entries}));
        }
    }
    Ok(result)
}

fn knowledge(call: &Call<'_>, window: &Window) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for file in call.ports.library.knowledge_pages()? {
        let text = markdown(&file)?;
        // Validate the header before trusting its identity. History selects
        // revisions even when a later update moved frontmatter out of period.
        metadata(text)?;
        let record = DocumentRecord::from_markdown(Path::new(&file.path), text);
        let mut revisions = Vec::new();
        for revision in call.ports.library.history(Path::new(&file.path))? {
            if window.contains(&revision.authored_at)? {
                revisions.push(json!({"commit":revision.commit,"at":revision.authored_at,"message":revision.message}));
            }
        }
        if !revisions.is_empty() {
            result.push(
                json!({"id":record.id,"title":record.title,"path":file.path,"revisions":revisions}),
            );
        }
    }
    Ok(result)
}
