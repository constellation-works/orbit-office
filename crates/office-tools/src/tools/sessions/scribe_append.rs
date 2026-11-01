//! `office.scribe-append`: validate scribe entries and append them to a
//! session's SCR file (specs/scribe.md, "Pipeline", "The SCR file").
//!
//! Model-free. Reads the message file `office.scribe-pending` wrote,
//! refuses (and reports) every entry whose quote is not an exact substring
//! of its message, whose tag or topic is not in the rules, or whose tag the
//! author may not take, then appends one line per accepted entry, merges
//! topics, advances `scribed_through` to `through` when one is supplied and
//! commits to the Library. A note/title-only call may omit `through` and
//! leaves `scribed_through` unchanged.

use office_core::domain::scribe_control::validate_session_id;
use office_core::domain::source::conversation::{
    self,
    scribe::{self, Parsed},
};
use office_core::domain::source::format_rfc3339_utc;
use office_core::ports::ScribeWrite;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "scribe-append",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "session_id",
        "through",
        "entries",
        "title",
        "title_by",
        "counterpart",
        "counterpart_name",
    ],
    run,
);

fn optional<'a>(call: &'a Call<'_>, key: &str) -> Result<Option<&'a str>> {
    match call.input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(Error::InvalidArgument(format!(
            "scribe-append {key} must be a string"
        ))),
    }
}

fn not_parsed() -> Error {
    Error::Library {
        code: "scribe_not_parsed",
        message: "no parsed messages for this session; run office.scribe-pending first".into(),
    }
}

fn run(call: &Call<'_>) -> Result<Value> {
    let session_id = optional(call, "session_id")?
        .ok_or_else(|| Error::InvalidArgument("scribe-append requires session_id".into()))?;
    validate_session_id(session_id)?;
    let through = optional(call, "through")?;
    let entries = scribe::parse_entries(call.input.get("entries"))?;
    let title = optional(call, "title")?;
    if through.is_none()
        && (entries.iter().any(|entry| entry.msg.is_some())
            || (entries.is_empty() && title.is_none()))
    {
        return Err(Error::InvalidArgument(
            "scribe-append requires through".into(),
        ));
    }
    let library = &call.ports.library;
    let parse = library
        .read_scribe_parse(session_id)?
        .ok_or_else(not_parsed)?;
    let messages = scribe::from_jsonl(&parse.messages)?;
    let parsed: Parsed = serde_json::from_str(&parse.meta).map_err(|_| not_parsed())?;
    if parsed.session_id != session_id {
        return Err(not_parsed());
    }
    let page = conversation::find_rules(&library.knowledge_pages()?)?;
    let rules = scribe::scribe_rules(page.as_ref(), parsed.project.as_deref());
    let files = scribe::scr_files(&library.scribe_files()?)?;
    let now = format_rfc3339_utc(call.ports.clock.now());
    let zone = call.ports.clock.time_zone();
    let request = scribe::Request {
        session_id,
        through,
        title,
        title_by: optional(call, "title_by")?,
        counterpart: optional(call, "counterpart")?,
        counterpart_name: optional(call, "counterpart_name")?,
        now: &now,
        zone: zone.as_deref(),
    };
    let plan = scribe::append(&request, &entries, &messages, &parsed, &rules, &files)?;
    let created = plan.previous.is_none();
    if let Some(markdown) = &plan.markdown {
        library.write_scribe_file(&ScribeWrite {
            path: plan.path.clone(),
            text: markdown.clone(),
            previous: plan.previous.clone(),
        })?;
    }
    Ok(json!({
        "session_id": session_id,
        "id": plan.id,
        "path": plan.path,
        "created": created && plan.markdown.is_some(),
        "written": plan.markdown.is_some(),
        "renamed_from": plan.renamed_from,
        "title": plan.front.title,
        "title_by": plan.front.title_by,
        "previous_through": plan.previous_through,
        "scribed_through": plan.front.scribed_through,
        "topics": plan.front.topics,
        "added": plan.added.iter().map(|(line, text)| json!({"line":line,"text":text})).collect::<Vec<_>>(),
        "duplicates": plan.duplicates,
        "refused": plan.refused,
    }))
}
