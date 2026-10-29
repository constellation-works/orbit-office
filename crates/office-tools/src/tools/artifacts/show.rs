//! Read an artifact's complete provenance, files, series and citing documents.
//!
//! A `private` artifact returns metadata only: its body companion is never
//! read out, and the output is marked `content_withheld: true`, so private
//! artifact text never reaches a caller of this MCP-visible tool (the
//! private-content decision in docs/design/office/4_decisions.md).

use std::path::Path;

use office_core::domain::artifact::{Artifact, ArtifactError, ArtifactKind, valid_id};
use office_core::domain::knowledge::DocumentRecord;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

/// Maximum artifact body bytes returned by `artifact-show`. A longer text
/// body is cut at this many bytes (backed off to a character boundary) and
/// reported with `truncated: true`.
pub(crate) const BODY_LIMIT_BYTES: usize = 16_384;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "artifact-show",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["id", "history"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let include_history = crate::history::requested(call.input)?;
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            valid_id(id.rsplit('/').next().unwrap_or_default())
                && office_core::domain::knowledge::parse_citation(id)
                    .is_some_and(|c| c.anchor.is_none())
        })
        .ok_or_else(|| {
            Error::InvalidArgument("artifact-show id must be art-YYYYMMDD-<slug>".into())
        })?;
    let library = &call.ports.library;
    let documents = library.artifact_documents()?;
    let record = library.get(id)?.ok_or(ArtifactError::NotFound)?;
    let path = &record.path;
    let text = library.read_text(Path::new(path))?;
    let artifact = Artifact::from_document(path, &text)?;
    let mut history = Vec::new();
    for file in &documents {
        let text = file.markdown.as_deref().ok_or(ArtifactError::Invalid {
            field: "frontmatter",
        })?;
        let version_record = DocumentRecord::from_markdown(Path::new(&file.path), text);
        if version_record.series.as_deref() == Some(&artifact.series)
            && version_record.layer == record.layer
            && version_record.shared_by == record.shared_by
        {
            let version = Artifact::from_document(&file.path, text)?;
            history.push((version, version_record));
        }
    }
    history.sort_by(|a, b| {
        b.0.version
            .cmp(&a.0.version)
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    let series_history: Vec<_> = history.iter().map(|(artifact, record)| {
        json!({"id":record.id,"version":artifact.version,"supersedes":artifact.supersedes,"path":record.path})
    }).collect();
    let dir = path
        .strip_suffix("/artifact.md")
        .ok_or(ArtifactError::Invalid { field: "path" })?;
    let files: Vec<_> = artifact
        .files
        .iter()
        .map(|name| json!({"name":name,"path":format!("{dir}/{name}")}))
        .collect();
    // The body is the first Markdown companion of a report or review, as
    // written (citations stay inline). Other kinds, a companion that is
    // not valid UTF-8 (a binary artifact), or no Markdown companion
    // return no body. A private artifact's body is withheld entirely.
    let withheld = artifact.sensitivity.as_str() == "private" || record.sensitivity == "private";
    let mut body: Option<String> = None;
    let mut truncated = false;
    if !withheld
        && matches!(artifact.kind, ArtifactKind::Report | ArtifactKind::Review)
        && let Some(name) = artifact.files.iter().find(|name| name.ends_with(".md"))
    {
        let bytes = library.read_artifact_file(Path::new(&format!("{dir}/{name}")))?;
        if let Ok(text) = String::from_utf8(bytes) {
            if text.len() > BODY_LIMIT_BYTES {
                let mut end = BODY_LIMIT_BYTES;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                body = Some(text[..end].to_string());
                truncated = true;
            } else {
                body = Some(text);
            }
        }
    }
    let mut output = json!({"layer":record.layer,"shared_by":record.shared_by,"artifact":artifact,"path":path,"files":files,"series_history":series_history,"backlinks":library.backlinks(id)?,"truncated":truncated});
    output["artifact"]["id"] = json!(record.id);
    if withheld {
        output["content_withheld"] = json!(true);
    }
    if let Some(body) = body {
        output["body"] = json!(body);
    }
    if include_history {
        crate::history::append(library.as_ref(), Path::new(path), &mut output)?;
    }
    Ok(output)
}
