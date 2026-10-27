//! `office.capture`: Create a source from an upload path or pasted text;
//! dedupe; return the id. With `append_to`, append conversation entries.

use office_core::domain::source::format_rfc3339_utc;
use office_core::ports::{CaptureSourceRequest, ConversationEntry};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "capture",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "kind",
        "title",
        "text",
        "upload_path",
        "path",
        "append_to",
        "entries",
        "entry_kind",
        "transcript_path",
        "origin",
        "sensitivity",
        "occurred_at",
        "participants",
        "captured_by",
    ],
    run,
);

fn string_field(call: &Call<'_>, key: &str) -> Result<Option<String>> {
    match call.input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(Error::InvalidArgument(format!(
            "capture {key} must be a string"
        ))),
    }
}

fn parse_entries(call: &Call<'_>) -> Result<Vec<ConversationEntry>> {
    let mut entries = Vec::new();
    if let Some(raw) = call.input.get("entries") {
        let items = raw.as_array().ok_or_else(|| {
            Error::InvalidArgument("capture entries must be an array".to_string())
        })?;
        for item in items {
            if let Some(text) = item.as_str() {
                let (kind, text) = text.split_once(':').ok_or_else(|| {
                    Error::InvalidArgument(
                        "capture entries strings must be \"<kind>: <text>\"".to_string(),
                    )
                })?;
                entries.push(ConversationEntry {
                    kind: kind.trim().to_string(),
                    text: text.trim().to_string(),
                });
            } else if let Some(object) = item.as_object() {
                let kind = object
                    .get("kind")
                    .or_else(|| object.get("type"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Error::InvalidArgument("capture entries items need a kind".to_string())
                    })?;
                let text = object
                    .get("text")
                    .or_else(|| object.get("message"))
                    .or_else(|| object.get("content"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Error::InvalidArgument("capture entries items need text".to_string())
                    })?;
                entries.push(ConversationEntry {
                    kind: kind.to_string(),
                    text: text.to_string(),
                });
            } else {
                return Err(Error::InvalidArgument(
                    "capture entries items must be objects or strings".to_string(),
                ));
            }
        }
    }
    if entries.is_empty()
        && let Some(kind) = string_field(call, "entry_kind")?
        && let Some(text) = string_field(call, "text")?
    {
        entries.push(ConversationEntry { kind, text });
    }
    Ok(entries)
}

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let upload_path = match string_field(call, "upload_path")? {
        Some(path) => Some(path),
        None => string_field(call, "path")?,
    };
    let entries = parse_entries(call)?;
    // `text` is the entry text only when entry_kind shaped the entry.
    let text = if !entries.is_empty() && call.input.contains_key("entry_kind") {
        None
    } else {
        string_field(call, "text")?
    };
    let participants = match call.input.get("participants") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    Error::InvalidArgument("capture participants must be strings".to_string())
                })
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "capture participants must be an array".to_string(),
            ));
        }
    };
    let request = CaptureSourceRequest {
        kind: string_field(call, "kind")?,
        title: string_field(call, "title")?,
        text,
        upload_path,
        transcript_path: string_field(call, "transcript_path")?,
        append_to: string_field(call, "append_to")?,
        entries,
        captured_at: format_rfc3339_utc(call.ports.clock.now()),
        captured_by: string_field(call, "captured_by")?,
        origin: string_field(call, "origin")?,
        occurred_at: string_field(call, "occurred_at")?,
        participants,
        sensitivity: string_field(call, "sensitivity")?,
    };
    if request.append_to.is_none()
        && request.text.is_none()
        && request.upload_path.is_none()
        && request.entries.is_empty()
    {
        return Err(Error::InvalidArgument(
            "capture requires text, upload_path, entries or append_to".to_string(),
        ));
    }
    let outcome = call.ports.library.capture_source(&request)?;
    Ok(json!({
        "id": outcome.id,
        "kind": outcome.kind,
        "title": outcome.title,
        "path": outcome.path,
        "sha256": outcome.sha256,
        "bytes": outcome.bytes,
        "media_type": outcome.media_type,
        "sensitivity": outcome.sensitivity,
        "deduplicated": outcome.deduplicated,
        "appended": outcome.appended,
        "version": outcome.version,
    }))
}
