//! Read an artifact's complete provenance, files, series and citing documents.

use std::path::Path;

use office_core::domain::artifact::{Artifact, ArtifactError, valid_id};
use office_core::domain::knowledge::DocumentRecord;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "artifact-show",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["id"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .ok_or_else(|| {
            Error::InvalidArgument("artifact-show id must be art-YYYYMMDD-<slug>".into())
        })?;
    let library = &call.ports.library;
    let documents = library.artifact_documents()?;
    let (path, text) = documents
        .iter()
        .find_map(|file| {
            let text = file.markdown.as_deref()?;
            (DocumentRecord::from_markdown(Path::new(&file.path), text).id == id)
                .then_some((&file.path, text))
        })
        .ok_or(ArtifactError::NotFound)?;
    let artifact = Artifact::from_document(path, text)?;
    let mut history = Vec::new();
    for file in &documents {
        let text = file.markdown.as_deref().ok_or(ArtifactError::Invalid {
            field: "frontmatter",
        })?;
        let record = DocumentRecord::from_markdown(Path::new(&file.path), text);
        if record.series.as_deref() == Some(&artifact.series) {
            let version = Artifact::from_document(&file.path, text)?;
            history.push(version);
        }
    }
    history.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| a.id.cmp(&b.id)));
    let series_history = history
        .iter()
        .map(|a| {
            Ok(json!({"id":a.id,"version":a.version,"supersedes":a.supersedes,"path":a.path()?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let dir = path
        .strip_suffix("/artifact.md")
        .ok_or(ArtifactError::Invalid { field: "path" })?;
    let files: Vec<_> = artifact
        .files
        .iter()
        .map(|name| json!({"name":name,"path":format!("{dir}/{name}")}))
        .collect();
    Ok(
        json!({"artifact":artifact,"path":path,"files":files,"series_history":series_history,"backlinks":library.backlinks(id)?}),
    )
}
