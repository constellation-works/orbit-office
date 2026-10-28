//! Approved, pinned script execution. Every external operation goes through ports.

use crate::registry::{Call, Tool};
use office_core::domain::artifact::{Artifact, ArtifactKind, decode_payload};
use office_core::domain::knowledge::{DocumentRecord, LibraryLayer};
use office_core::domain::script::{ScriptError, ScriptExecution, ScriptPreparation, ScriptRequest};
use office_core::ports::Ports;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "script-run",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["artifact", "sha256", "inputs", "args", "output"],
    run,
);

fn script(ports: &Ports<'_>, request: &ScriptRequest) -> Result<(ScriptPreparation, Vec<String>)> {
    request.validate()?;
    let documents = ports.library.artifact_documents()?;
    let find = |id: &str| -> Result<Artifact> {
        let document = documents
            .iter()
            .find(|doc| {
                doc.markdown.as_deref().is_some_and(|text| {
                    let record = DocumentRecord::from_markdown(Path::new(&doc.path), text);
                    record.layer == LibraryLayer::Own && record.id == id
                })
            })
            .ok_or(ScriptError::NotApproved)?;
        Artifact::from_document(
            &document.path,
            document
                .markdown
                .as_deref()
                .ok_or(ScriptError::NotApproved)?,
        )
        .map_err(|_| ScriptError::NotApproved.into())
    };
    let artifact = find(&request.artifact)?;
    if artifact.kind != ArtifactKind::Script || artifact.files.len() != 1 {
        return Err(ScriptError::NotApproved.into());
    }
    let path = artifact.path()?;
    let directory = path
        .strip_suffix("artifact.md")
        .ok_or(ScriptError::NotApproved)?;
    let mut inputs = Vec::new();
    for input in &request.inputs {
        if input.starts_with("art-") {
            let artifact = find(input)?;
            let path = artifact.path()?;
            let dir = path
                .strip_suffix("artifact.md")
                .ok_or(ScriptError::NotApproved)?;
            inputs.extend(artifact.files.iter().map(|file| format!("{dir}{file}")));
        } else {
            inputs.push(input.clone());
        }
    }
    Ok((
        ScriptPreparation {
            path: format!("{directory}{}", artifact.files[0]),
            sha256: request.sha256.clone(),
        },
        inputs,
    ))
}

/// Proposal acceptance calls this *before* enabling its automation. It validates
/// script kind/hash and fetches dependencies without requiring a done proposal.
pub fn prepare_script(ports: &Ports<'_>, request: &ScriptRequest) -> Result<()> {
    let (script, _) = script(ports, request)?;
    ports.scripts.prepare(&script)
}

fn run(call: &Call<'_>) -> Result<Value> {
    let request: ScriptRequest = serde_json::from_value(Value::Object(call.input.clone()))
        .map_err(|_| {
            Error::InvalidArgument(
                "script-run expects artifact, sha256, inputs, args and a closed output mapping"
                    .into(),
            )
        })?;
    let (script, inputs) = script(call.ports, &request)?;
    approved(call, &request)?;
    let bounds = call.ports.library.settings()?.scripts;
    let run_id = call
        .context
        .get("run_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let execution = ScriptExecution {
        request,
        script,
        inputs,
        run_id,
    };
    let (mut result, files) = call.ports.scripts.run(&execution, &bounds)?;
    if result.exit_code != Some(0) {
        return Err(ScriptError::Exit {
            exit_code: result.exit_code,
            log_tail: result.log_tail,
        }
        .into());
    }
    if !files.is_empty() {
        let filed = (|| -> Result<String> {
            let task = call.ports.orbit.task_add(&object(json!({
                "title":execution.request.output.title, "type":"feature", "complexity":"low", "origin":"automation",
                "tags":["script-output"], "description":"Script output awaiting review and artifact promotion.",
            }))?)?;
            let id = task
                .get("task")
                .unwrap_or(&task)
                .get("id")
                .and_then(Value::as_str)
                .ok_or(Error::OrbitOutcomeUnknown)?;
            for file in &files {
                call.ports.orbit.task_artifact_put(&object(
                    json!({"id":id,"path":file.path,"source_path":file.source_path}),
                )?)?;
            }
            Ok(id.into())
        })();
        // All originals and manifests remain in the retained run directory.
        // Attempt every staging cleanup, including after an uncertain callback.
        let mut cleanup = Ok(());
        for file in &files {
            let removed = call.ports.library.remove_staged(&file.source_path);
            if cleanup.is_ok() {
                cleanup = removed;
            }
        }
        let id = filed?;
        cleanup?;
        result.output_task = Some(id);
    }
    Ok(json!(result))
}

fn approved(call: &Call<'_>, request: &ScriptRequest) -> Result<()> {
    let listed = call.ports.orbit.task_list(&object(
        json!({"tag":"proposal","status":"done","limit":1000,"fields":["id","status","tags","execution_summary"]}),
    )?)?;
    if listed.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(ScriptError::NotApproved.into());
    }
    let tasks = listed
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or(ScriptError::NotApproved)?;
    for task in tasks {
        let tags = task.get("tags").and_then(Value::as_array);
        let has_tag = |tag| tags.is_some_and(|tags| tags.iter().any(|v| v.as_str() == Some(tag)));
        if task.get("status").and_then(Value::as_str) != Some("done")
            || !has_tag("proposal")
            || !(has_tag("proposal:routine") || has_tag("proposal:auto_task"))
        {
            continue;
        }
        let id = task
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ScriptError::NotApproved)?;
        let payload = call
            .ports
            .orbit
            .task_artifact_get(&object(json!({"id":id,"path":"proposal.json"}))?)?;
        let bytes = decode_payload(&payload)?;
        if let Some(digest) = task
            .get("execution_summary")
            .and_then(Value::as_str)
            .and_then(|summary| {
                summary.strip_prefix(office_core::domain::proposal::APPROVAL_DIGEST_PREFIX)
            })
            .and_then(|rest| rest.lines().next())
            && office_core::domain::source::sha256_hex(&bytes) != digest
        {
            continue;
        }
        let payload: Value =
            serde_json::from_slice(&bytes).map_err(|_| ScriptError::NotApproved)?;
        let kind = payload.get("kind").and_then(Value::as_str);
        if !matches!(kind, Some("routine" | "auto_task")) {
            continue;
        }
        // A pin counts only as the actual input of a plugin.tool_call step,
        // never as arbitrary prose or an unrelated artifact/hash pair.
        if payload
            .get("definition")
            .is_some_and(|definition| names_step(definition, request))
        {
            return Ok(());
        }
    }
    Err(ScriptError::NotApproved.into())
}
fn names_step(value: &Value, request: &ScriptRequest) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("plugin.tool_call")
                && map.get("tool").and_then(Value::as_str) == Some("office.script-run")
                && map
                    .get("input")
                    .cloned()
                    .and_then(|input| serde_json::from_value::<ScriptRequest>(input).ok())
                    .is_some_and(|input| json!(input) == json!(request))
            {
                return true;
            }
            map.values().any(|v| names_step(v, request))
        }
        Value::Array(items) => items.iter().any(|v| names_step(v, request)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}
fn object(value: Value) -> Result<Map<String, Value>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| Error::InvalidArgument("expected a callback mapping".into()))
}
