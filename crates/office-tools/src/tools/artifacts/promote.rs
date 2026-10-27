//! Copy reviewed task outputs through Orbit, then publish one Library bundle.
//! This MCP-hidden approval action stamps user provenance; caller input never
//! chooses the approver, production crew, approval time, id or version.

use office_core::domain::artifact::{
    Approval, Approver, Artifact, ArtifactError, ArtifactFile, ArtifactManifest, decode_payload,
};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "artifact-promote",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["task", "manifest", "edited"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let task = call
        .input
        .get("task")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Error::InvalidArgument("artifact-promote task must be a task id".into()))?;
    let manifest_path = match call.input.get("manifest") {
        None => "artifact-manifest.json",
        Some(Value::String(path)) if !path.is_empty() => path,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "artifact-promote manifest must be an artifact path".into(),
            ));
        }
    };
    let edited = match call.input.get("edited") {
        None => false,
        Some(Value::Bool(edited)) => *edited,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "artifact-promote edited must be boolean".into(),
            ));
        }
    };
    let shown = match call.ports.orbit.task_show(
        &json!({"id":task,"fields":["id","status","artifacts","crew","updated_at"]})
            .as_object()
            .cloned()
            .ok_or(ArtifactError::Payload)?,
    ) {
        Ok(value) => value,
        Err(Error::OrbitRefused { code }) if code == "task_not_found" => {
            return Err(Error::TaskNotFound { id: task.into() });
        }
        Err(error) => return Err(error),
    };
    let record = shown.get("task").unwrap_or(&shown);
    if !matches!(
        record.get("status").and_then(Value::as_str),
        Some("review" | "done")
    ) {
        return Err(ArtifactError::NotReviewable.into());
    }
    let artifacts = record
        .get("artifacts")
        .and_then(Value::as_array)
        .ok_or(ArtifactError::Payload)?;
    if artifacts.is_empty() {
        return Err(ArtifactError::NoArtifacts.into());
    }
    let exists = |path: &str| {
        artifacts
            .iter()
            .any(|file| file.get("path").and_then(Value::as_str) == Some(path))
    };
    if !exists(manifest_path) {
        return Err(ArtifactError::Invalid { field: "manifest" }.into());
    }
    let manifest_bytes = get_file(call, task, manifest_path)?;
    let mut manifest: ArtifactManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| ArtifactError::Invalid { field: "manifest" })?;
    manifest.validate()?;
    // Sensitivity is inherited from cited Library evidence, never reduced by
    // the output manifest. An unresolved citation refuses before any write.
    for cite in manifest.cites.clone() {
        let id = cite
            .split('#')
            .next()
            .ok_or(ArtifactError::Invalid { field: "cites" })?;
        let cited = call
            .ports
            .library
            .get(id)?
            .ok_or(ArtifactError::Invalid { field: "cites" })?;
        manifest.inherit_sensitivity(&cited.sensitivity)?;
    }
    if manifest
        .files
        .iter()
        .any(|path| !exists(path) || path == manifest_path)
    {
        return Err(ArtifactError::Invalid { field: "files" }.into());
    }
    let produced_by = office_core::domain::artifact::Producer {
        crew: record
            .get("crew")
            .and_then(Value::as_str)
            .map(str::to_string),
        runbook: manifest.runbook.clone(),
        at: record
            .get("updated_at")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let artifact = Artifact::approved(
        manifest,
        task.into(),
        produced_by,
        Approval {
            by: Approver::User,
            at: format_rfc3339_utc(call.ports.clock.now()),
            edited,
        },
    )?;
    // Read every file first: a late callback refusal leaves no partial Library.
    let files = artifact
        .files
        .iter()
        .map(|path| {
            Ok(ArtifactFile {
                path: path.clone(),
                bytes: get_file(call, task, path)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let artifact = call.ports.library.promote_artifact(&artifact, &files)?;
    Ok(
        json!({"id":artifact.id,"path":artifact.path()?,"files":artifact.files,
        "series":artifact.series,"version":artifact.version,"supersedes":artifact.supersedes}),
    )
}

fn get_file(call: &Call<'_>, task: &str, path: &str) -> Result<Vec<u8>> {
    let mut input = Map::new();
    input.insert("id".into(), json!(task));
    input.insert("path".into(), json!(path));
    Ok(decode_payload(
        &call.ports.orbit.task_artifact_get(&input)?,
    )?)
}
