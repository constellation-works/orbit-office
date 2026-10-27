//! List the complete artifact inventory, filterable by kind and series.
//!
//! This is an all-or-nothing, bounded read: an unreadable or invalid
//! artifact refuses the request rather than imply that an output is
//! absent. `office.search` refuses empty queries and `artifact-show`
//! needs a known id, so this tool is the Library screen's inventory.

use office_core::domain::artifact::{Artifact, ArtifactKind};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "artifact-list",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["kind", "series", "limit"],
    run,
);

/// Artifacts returned when `limit` is absent, and the most a call may ask for.
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

fn run(call: &Call<'_>) -> Result<Value> {
    let kind = match call.input.get("kind") {
        None => None,
        Some(Value::String(token)) => Some(parse_kind(token)?),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "artifact-list kind must be report, doc, draft, script, data, review or other"
                    .into(),
            ));
        }
    };
    let series = match call.input.get("series") {
        None => None,
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "artifact-list series must be nonempty text".into(),
            ));
        }
    };
    let limit = match call.input.get("limit") {
        None => DEFAULT_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_LIMIT).contains(limit))
            .ok_or_else(|| {
                Error::InvalidArgument(format!(
                    "artifact-list limit must be an integer from 1 to {MAX_LIMIT}"
                ))
            })?,
    };
    let library = &call.ports.library;
    let mut artifacts = Vec::new();
    for file in library.artifact_documents()? {
        let text = file.markdown.as_deref().ok_or_else(|| Error::Library {
            code: "library_document_unreadable",
            message: format!("cannot read {}", file.path),
        })?;
        artifacts.push(Artifact::from_document(&file.path, text)?);
    }
    // Newest first by approval time; the id breaks ties deterministically.
    artifacts.sort_by(|a, b| {
        b.approved
            .at
            .cmp(&a.approved.at)
            .then_with(|| b.id.cmp(&a.id))
    });
    let filtered: Vec<_> = artifacts
        .iter()
        .filter(|a| kind.is_none_or(|k| a.kind == k))
        .filter(|a| series.is_none_or(|s| a.series == s))
        .collect();
    let total = filtered.len();
    let truncated = total > limit;
    let artifacts: Vec<_> = filtered
        .iter()
        .take(limit)
        .map(|a| {
            json!({
                "id": a.id,
                "kind": kind_token(a.kind),
                "series": a.series,
                "title": a.title,
                "approved_at": a.approved.at,
                "sensitivity": a.sensitivity.as_str(),
            })
        })
        .collect();
    Ok(json!({"artifacts": artifacts, "total": total, "truncated": truncated}))
}

fn parse_kind(token: &str) -> Result<ArtifactKind> {
    match token {
        "report" => Ok(ArtifactKind::Report),
        "doc" => Ok(ArtifactKind::Doc),
        "draft" => Ok(ArtifactKind::Draft),
        "script" => Ok(ArtifactKind::Script),
        "data" => Ok(ArtifactKind::Data),
        "review" => Ok(ArtifactKind::Review),
        "other" => Ok(ArtifactKind::Other),
        _ => Err(Error::InvalidArgument(
            "artifact-list kind must be report, doc, draft, script, data, review or other".into(),
        )),
    }
}

fn kind_token(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Report => "report",
        ArtifactKind::Doc => "doc",
        ArtifactKind::Draft => "draft",
        ArtifactKind::Script => "script",
        ArtifactKind::Data => "data",
        ArtifactKind::Review => "review",
        ArtifactKind::Other => "other",
    }
}
