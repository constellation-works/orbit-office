//! `office.source-show`: Return a source, optionally at an anchor.

use std::path::Path;

use office_core::domain::source::{access, anchor_span, parse_anchor};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "source-show",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "id",
        "source_id",
        "anchor",
        "history",
        "media",
        "frame",
        "document",
    ],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let history = crate::history::requested(call.input)?;
    let media = media_selection(call)?;
    let id = match (call.input.get("id"), call.input.get("source_id")) {
        (Some(Value::String(id)), _) if !id.trim().is_empty() => id.clone(),
        (_, Some(Value::String(id))) if !id.trim().is_empty() => id.clone(),
        _ => {
            return Err(Error::InvalidArgument(
                "source-show requires a source id".to_string(),
            ));
        }
    };
    let anchor = match call.input.get("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "source-show anchor must be a string".to_string(),
            ));
        }
    };
    let view = call.ports.library.read_source(&id)?;
    let markdown = call.ports.library.read_text(Path::new(&view.path))?;
    let record = office_core::domain::knowledge::DocumentRecord::from_markdown(
        Path::new(&view.path),
        &markdown,
    );
    let derived_by = access::derived_by(&markdown)?;
    let chapters = call
        .ports
        .library
        .source_sidecar(&id, "chapters.md")?
        .map(|text| access::chapters(&text))
        .transpose()?
        .unwrap_or_default();
    let frames = call
        .ports
        .library
        .source_sidecar(&id, "frames.md")?
        .map(|text| access::frames(&text))
        .transpose()?
        .unwrap_or_default();
    let gotchas = call
        .ports
        .library
        .source_sidecar(&id, "digest.json")?
        .map(|text| access::gotchas(&text))
        .transpose()?
        .unwrap_or_default();
    let summary = call.ports.library.source_sidecar(&id, "summary.md")?;
    let selected_body = match call.input.get("document") {
        None | Some(Value::Null) => &view.body,
        Some(Value::String(name)) if name == "source" => &view.body,
        Some(Value::String(name)) if name == "summary" => {
            summary.as_ref().ok_or(Error::Library {
                code: "summary_missing",
                message: "this source has no summary".into(),
            })?
        }
        Some(_) => {
            return Err(Error::InvalidArgument(
                "document must be source or summary".into(),
            ));
        }
    };
    let (cited_by, task_citers_available, cited_by_truncated) = citers(call, &id)?;
    let (span, resolved_anchor) = match &anchor {
        Some(raw) => {
            let parsed = parse_anchor(raw)?;
            (
                Some(anchor_span(selected_body, parsed, raw)?),
                Some(raw.clone()),
            )
        }
        None => (None, None),
    };
    let body = span.clone().unwrap_or_else(|| selected_body.clone());
    let mut output = json!({
        "id": view.id,
        "layer": record.layer,
        "shared_by": record.shared_by,
        "kind": view.kind,
        "title": view.title,
        "path": view.path,
        "body": body,
        "text": body,
        "span": span,
        "anchor": resolved_anchor,
        "sensitivity": view.sensitivity,
        "sha256": view.sha256,
        "bytes": view.bytes,
        "media_type": view.media_type,
        "version": view.version,
        "derived_by": derived_by,
        "cited_by": cited_by,
        "task_citers_available": task_citers_available,
        "cited_by_truncated": cited_by_truncated,
        "chapters": chapters,
        "frames": frames,
        "gotchas": gotchas,
        "summary": summary,
    });
    if let Some(selection) = media {
        let media = call.ports.library.source_media(&id, &selection)?;
        let too_large = media.content.is_none();
        output["media"] = json!({
            "name":media.name, "media_type":media.media_type, "bytes":media.bytes,
            "content_base64":media.content.as_deref().map(access::base64),
            "limit_bytes":access::MEDIA_LIMIT, "too_large":too_large, "truncated":too_large,
        });
    }
    if history {
        crate::history::append(
            call.ports.library.as_ref(),
            Path::new(&view.path),
            &mut output,
        )?;
    }
    Ok(output)
}

fn media_selection(call: &Call<'_>) -> Result<Option<access::MediaSelection>> {
    match (call.input.get("media"), call.input.get("frame")) {
        (None, None) => Ok(None),
        (Some(Value::String(mode)), None) if mode == "original" => {
            Ok(Some(access::MediaSelection::Original))
        }
        (Some(Value::String(mode)), Some(Value::String(name))) if mode == "frame" => {
            let selection = access::MediaSelection::Frame(name.clone());
            // Check only the selection's shape here; the adapter checks actual
            // metadata and private-route policy under its media read guard.
            access::frame_file(name)?;
            Ok(Some(selection))
        }
        _ => Err(Error::InvalidArgument(
            "media must be original, or frame with a frame PNG basename".into(),
        )),
    }
}

fn citers(call: &Call<'_>, id: &str) -> Result<(Vec<Value>, bool, bool)> {
    let mut citers = call
        .ports
        .library
        .backlinks(id)?
        .into_iter()
        .map(|link| json!({"id":link.id,"title":link.title,"kind":link.kind.as_str()}))
        .collect::<Vec<_>>();
    let input = json!({"tag":format!("source:{id}"),"limit":100,"fields":["id","title"]});
    let response = match call
        .ports
        .orbit
        .task_list(input.as_object().ok_or(Error::OrbitUnavailable)?)
    {
        Ok(value) => value,
        // Task citations are an explicitly marked optional read: a standalone
        // source display also works without a live callback session or grant.
        Err(_) => return Ok((citers, false, false)),
    };
    let invalid = || Error::Library {
        code: "source_citers_invalid",
        message: "invalid citing-task response from Orbit".into(),
    };
    let tasks = response["tasks"].as_array().ok_or_else(invalid)?;
    let truncated = response["truncated"].as_bool().ok_or_else(invalid)?;
    let total = response["total"].as_u64().ok_or_else(invalid)?;
    let count = u64::try_from(tasks.len()).map_err(|_| invalid())?;
    if count > 100 || total < count || truncated != (total > count) {
        return Err(invalid());
    }
    for task in tasks {
        let id = task["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        let title = task["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        citers.push(json!({"id":id,"title":title,"kind":"task"}));
    }
    citers.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    citers.dedup_by(|a, b| a["id"] == b["id"] && a["kind"] == b["kind"]);
    Ok((citers, true, truncated))
}
