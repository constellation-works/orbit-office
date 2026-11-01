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
        "session_transcript_path",
        "session_id",
        "transcript_preview",
        "transcript_sha256",
        "selections",
        "from_line",
        "handoff",
        "project",
        "branch",
        "resume",
        "repository",
        "title_by",
        "event_key",
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
        "tags",
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
                    by: None,
                });
            } else if item.is_object() {
                entries.push(
                    office_core::domain::source::conversation::corrections::capture_entry(item)?,
                );
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
        entries.push(ConversationEntry {
            kind,
            text,
            by: None,
        });
    }
    Ok(entries)
}

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    if [
        "session_transcript_path",
        "session_id",
        "transcript_preview",
        "transcript_sha256",
        "selections",
        "from_line",
        "handoff",
        "project",
        "branch",
        "resume",
        "repository",
        "title_by",
    ]
    .iter()
    .any(|key| call.input.contains_key(*key))
    {
        return capture_session(call);
    }
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
    let mut request = CaptureSourceRequest {
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
        event: None,
        tags: match call.input.get("tags") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_string).ok_or_else(|| {
                        Error::InvalidArgument("capture tags must be strings".into())
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            Some(_) => {
                return Err(Error::InvalidArgument(
                    "capture tags must be an array".into(),
                ));
            }
        },
        sensitivity: string_field(call, "sensitivity")?,
    };
    if let Some(key) = string_field(call, "event_key")? {
        if request.append_to.is_some() || !request.entries.is_empty() {
            return Err(Error::InvalidArgument(
                "event_key cannot be combined with conversation entries or append_to".into(),
            ));
        }
        let (snapshot, _) = call.ports.calendar.load()?;
        let event = snapshot
            .events
            .iter()
            .find(|event| event.key == key)
            .ok_or(Error::Library {
                code: "unknown_event",
                message: "no selected calendar event has this key; check calendar choices or sync the connected calendar".into(),
            })?;
        request.kind = Some("meeting".into());
        request.title = Some(event.title.clone());
        request.occurred_at = Some(event.start.clone());
        request.participants = event.attendees.iter().map(|a| a.name.clone()).collect();
        request.sensitivity = Some(event.sensitivity.clone());
        request.event = Some(office_core::domain::source::SourceEvent {
            key: event.key.clone(),
            series: event.series.clone(),
        });
    }
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

fn capture_session(call: &Call<'_>) -> Result<Value> {
    use office_core::domain::source::conversation::{self, Request};
    if call.input.keys().any(|key| {
        ![
            "session_transcript_path",
            "session_id",
            "transcript_preview",
            "transcript_sha256",
            "selections",
            "from_line",
            "handoff",
            "project",
            "branch",
            "resume",
            "repository",
            "title",
            "title_by",
        ]
        .contains(&key.as_str())
    }) {
        return Err(Error::InvalidArgument(
            "session transcript capture cannot be combined with other capture fields".into(),
        ));
    }
    let path = string_field(call, "session_transcript_path")?.ok_or_else(|| {
        Error::InvalidArgument(
            "session_transcript_path and session_id are required together".into(),
        )
    })?;
    let session_id = string_field(call, "session_id")?.ok_or_else(|| {
        Error::InvalidArgument(
            "session_transcript_path and session_id are required together".into(),
        )
    })?;
    let from_line = match call.input.get("from_line") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| {
                    Error::InvalidArgument(
                        "capture from_line must be a non-negative integer".into(),
                    )
                })?,
        ),
    };
    let preview = match call.input.get("transcript_preview") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "transcript_preview must be a boolean".into(),
            ));
        }
    };
    if preview {
        if [
            "selections",
            "transcript_sha256",
            "handoff",
            "project",
            "branch",
            "resume",
            "repository",
            "title",
            "title_by",
        ]
        .iter()
        .any(|key| call.input.contains_key(*key))
        {
            return Err(Error::InvalidArgument(
                "preview cannot accept extraction or session context fields".into(),
            ));
        }
        let session = call.ports.library.session_transcript(&path, &session_id)?;
        let lines = match from_line {
            None => session.lines.clone(),
            Some(from) => {
                let mut lines: Vec<_> = session
                    .lines
                    .iter()
                    .filter(|line| line.line < from && line.role == "assistant")
                    .max_by_key(|line| line.line)
                    .map(|context| {
                        let mut context = context.clone();
                        context.context = true;
                        vec![context]
                    })
                    .unwrap_or_default();
                lines.extend(
                    session
                        .lines
                        .iter()
                        .filter(|line| line.line >= from)
                        .cloned(),
                );
                lines
            }
        };
        // The rules page and the source cursor are data for the crew: where
        // the next pass starts, which anchor its first entry takes, and what
        // the user wants noted. Neither widens the guard capture applies.
        let rules = conversation::find_rules(&call.ports.library.knowledge_pages()?)?;
        let source =
            conversation::cursor(&call.ports.library.source_documents()?, &session.session_id)?;
        return Ok(
            json!({"session_id":session.session_id,"orchestrator":session.orchestrator,"model":session.model,"transcript_sha256":session.transcript_sha256,"sensitivity":session.sensitivity,"content_withheld":session.sensitivity == "private","lines":lines,"rules":rules,"source":source}),
        );
    }
    let selections = call
        .input
        .get("selections")
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|_| {
                Error::InvalidArgument(
                    "selections must be an array of {line, kind, answers?} with no other keys"
                        .into(),
                )
            })
        })
        .transpose()?;
    let handoff = string_field(call, "handoff")?;
    if handoff.is_some() && selections.is_none() {
        return Err(Error::InvalidArgument(
            "handoff requires selections; only a crew pass writes a scribe handoff".into(),
        ));
    }
    // The scribe handoff is off unless the rules page turns it on; a
    // handoff offered while it is off is dropped, never written.
    let enabled = match handoff {
        Some(_) => conversation::find_rules(&call.ports.library.knowledge_pages()?)?
            .is_some_and(|rules| rules.handoff),
        None => false,
    };
    let offered = handoff.is_some();
    let request = Request {
        path,
        session_id,
        selections,
        expected_sha256: string_field(call, "transcript_sha256")?,
        from_line,
        handoff: handoff.filter(|_| enabled),
        context: serde_json::from_value(Value::Object(
            [
                "project",
                "branch",
                "resume",
                "repository",
                "title",
                "title_by",
            ]
            .into_iter()
            .filter_map(|key| call.input.get(key).map(|v| (key.into(), v.clone())))
            .collect(),
        ))
        .map_err(|_| Error::InvalidArgument("invalid session context fields".into()))?,
    };
    if request.selections.is_none() && request.expected_sha256.is_some() {
        return Err(Error::InvalidArgument(
            "transcript_sha256 requires selections".into(),
        ));
    }
    let outcome = call
        .ports
        .library
        .capture_session(&request, &format_rfc3339_utc(call.ports.clock.now()))?;
    let mut response = json!({"id":outcome.id,"kind":outcome.kind,"title":outcome.title,"path":outcome.path,"sha256":outcome.sha256,"bytes":outcome.bytes,"media_type":outcome.media_type,"sensitivity":outcome.sensitivity,"deduplicated":outcome.deduplicated,"appended":outcome.appended,"version":outcome.version});
    if offered {
        response["handoff"] = json!(outcome.handoff.unwrap_or_else(|| "disabled".into()));
    }
    Ok(response)
}
