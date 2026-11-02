//! `office.scribe-pending`: the session messages a scribe pass has not read
//! yet (specs/scribe.md, "Pipeline").
//!
//! Model-free. Parses the transcript through the conversation adapter
//! (text between the user and the agent only, credentials scrubbed),
//! writes the private `<library>/.office/state/scribe/<session-id>.jsonl`
//! (git-ignored Library state) with stable `m-<n>` ids, and returns the SCR
//! file's metadata (or the name a new one takes), `scribed_through`, the
//! messages after it with up to 10 earlier messages as context, the
//! scribe rules, and the tags in force and the cadence (`scribe` settings,
//! specs/scribe.md#configuration). No Library commit.
//!
//! `format: "v2"` (the default once the session has a v2 SCR) adds the
//! model-free markers (the trust rule: `markers` from the user's typed
//! turns, `agent_markers` recorded `by: agent`), the session scope, the
//! scope's latest SCR as `previous`, and the session's v2 SCR (or the
//! name it takes). A private session's parse is still written locally;
//! its messages are withheld and its markers returned by reference.

use office_core::domain::scribe_control::validate_session_id;
use office_core::domain::source::conversation::{
    self,
    scribe::{self, CONTEXT_MESSAGES, PROVIDER, Parsed, v2},
};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "scribe-pending",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "session_id",
        "transcript",
        "provider",
        "cwd",
        "project",
        "custom_title",
        "session_title",
        "format",
        "scope",
    ],
    run,
);

fn required(call: &Call<'_>, key: &str) -> Result<String> {
    call.input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            !value.trim().is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::InvalidArgument(format!(
                "scribe-pending requires {key} as a nonempty single-line string"
            ))
        })
}

/// Optional caller-resolved project (the git repository name). The
/// plugin resolves it from the repository's main worktree, since `cwd`
/// alone names a linked worktree's directory; absent, the project falls
/// back to the `cwd` basename (a non-git directory, or a worktree git
/// can no longer resolve).
fn optional_project(call: &Call<'_>) -> Result<Option<String>> {
    match call.input.get("project") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if !value.trim().is_empty()
                && value.len() <= 4096
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(Error::InvalidArgument(
            "scribe-pending project must be a nonempty single-line string".into(),
        )),
    }
}

/// Optional caller-passed latest `customTitle` (the transcript's
/// `custom-title` record; `session_title` is accepted as its alias).
/// When present it is the session title for every session, ahead of
/// any title record the adapter read.
fn optional_title(call: &Call<'_>) -> Result<Option<String>> {
    for key in ["custom_title", "session_title"] {
        match call.input.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::String(value))
                if !value.trim().is_empty()
                    && value.len() <= 4096
                    && !value.chars().any(char::is_control) =>
            {
                return Ok(Some(value.trim().to_owned()));
            }
            Some(_) => {
                return Err(Error::InvalidArgument(format!(
                    "scribe-pending {key} must be a nonempty single-line string"
                )));
            }
        }
    }
    Ok(None)
}

fn optional_text<'a>(call: &'a Call<'_>, key: &str) -> Result<Option<&'a str>> {
    match call.input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(Error::InvalidArgument(format!(
            "scribe-pending {key} must be a string"
        ))),
    }
}

fn run(call: &Call<'_>) -> Result<Value> {
    let session_id = required(call, "session_id")?;
    validate_session_id(&session_id)?;
    let transcript = required(call, "transcript")?;
    let provider = required(call, "provider")?;
    let cwd = required(call, "cwd")?;
    if provider != PROVIDER {
        return Err(Error::Library {
            code: "unsupported_provider",
            message: "only claude-code transcripts have a conversation adapter".into(),
        });
    }
    let library = &call.ports.library;
    let session = library.session_transcript(&transcript, &session_id)?;
    let withheld = session.sensitivity == "private";
    let messages = scribe::messages(&session);
    let project = match optional_project(call)? {
        Some(project) => Some(project),
        None => scribe::project_of(&cwd),
    };
    let mut parsed = Parsed {
        session_id: session_id.clone(),
        provider: provider.clone(),
        model: session.model.clone(),
        host: library.host_name(),
        project: project.clone(),
        cwd: Some(cwd),
        branch: session.branch.clone(),
        transcript,
        parsed: library.scribe_parse_path(&session_id)?,
        started_at: session.started_at.clone(),
        session_title: session.title.clone(),
        custom_title: optional_title(call)?.or_else(|| session.custom_title.clone()),
        scope: None,
        private: false,
    };
    let v2_files = v2::v2_files(&library.scribe_files()?)?;
    let v2_record = v2::find_session(&v2_files, &session_id)?;
    let requested_scope = optional_text(call, "scope")?;
    match optional_text(call, "format")? {
        Some("v2") => {}
        None if v2_record.is_some() => {}
        Some("v1") | None if requested_scope.is_some() => {
            return Err(Error::InvalidArgument(
                "scribe-pending scope needs format v2".into(),
            ));
        }
        Some("v1") | None => {
            return v1(call, &session_id, &provider, &session, &messages, parsed);
        }
        Some(_) => {
            return Err(Error::InvalidArgument(
                "scribe-pending format is v1 or v2".into(),
            ));
        }
    }
    let scope = match (v2_record, requested_scope) {
        (Some(file), Some(requested))
            if v2::session_scope(project.as_deref(), Some(requested))?
                != file.front.scope_token() =>
        {
            return Err(Error::Library {
                code: "scribe_scope_fixed",
                message: "the session's SCR already has another scope".into(),
            });
        }
        (Some(file), _) => file.front.scope_token(),
        (None, requested) => v2::session_scope(project.as_deref(), requested)?,
    };
    // Private: credentials in the transcript, or the conversation
    // sensitivity setting (specs/scribe.md, "Privacy").
    let library_settings = library.settings()?;
    let withheld = withheld
        || library_settings
            .sensitivity
            .get("conversation")
            .is_some_and(|s| *s == office_core::ports::Sensitivity::Private);
    parsed.scope = Some(scope.clone());
    parsed.private = withheld;
    // Markers are detected locally even in a private session: its lines
    // go only to the private parse and are never returned.
    let messages = if session.sensitivity == "private" {
        scribe::messages(&library.scribe_transcript(&parsed.transcript, &session_id)?)
    } else {
        messages
    };
    let settings = library_settings.scribe;
    let effective = settings.effective();
    let tags = settings.effective_tags();
    let through = match v2_record.and_then(|f| f.front.scribed_through.as_deref()) {
        None => None,
        Some(id) => {
            let index = scribe::message_index(id).ok_or_else(|| Error::Library {
                code: "scribe_file_invalid",
                message: "the SCR file's scribed_through is not a message id".into(),
            })?;
            if index >= messages.len() {
                return Err(Error::Library {
                    code: "scribe_transcript_shrank",
                    message: "the transcript has fewer messages than scribed_through".into(),
                });
            }
            Some(index)
        }
    };
    let meta = serde_json::to_string(&parsed).map_err(|_| Error::Library {
        code: "scribe_parse_invalid",
        message: "session facts could not be serialized".into(),
    })?;
    let parse_path =
        library.write_scribe_parse(&session_id, &scribe::to_jsonl(&messages)?, &meta)?;
    let start = through.map_or(0, |index| index + 1);
    let last = messages.len().checked_sub(1);
    let (user, agent) = v2::detect(&messages, start, last, &tags, &scope);
    let marker_list = |list: &[v2::Marker]| -> Vec<Value> {
        list.iter()
            .map(|m| if withheld { m.reference() } else { json!(m) })
            .collect()
    };
    let zone = call.ports.clock.time_zone();
    let record = match v2_record {
        Some(file) => json!({
            "exists": true, "id": file.front.id, "path": file.path, "title": file.front.title,
            "title_by": file.front.title_by, "scope": file.front.scope_token(),
            "previous": file.front.previous, "scribed_through": file.front.scribed_through,
            "updated": file.front.updated, "sensitivity": file.front.sensitivity,
            "sections": if withheld { Value::Null } else { sections(&file.sections) },
            "tag_entries": file.entries().len(), "next_tag": format!("T{}", file.next_tag_number()),
        }),
        None => match parsed
            .started_at
            .clone()
            .or_else(|| messages.first().map(|m| m.at.clone()))
        {
            None => Value::Null,
            Some(at) => {
                let start = scribe::local(&at, zone.as_deref())?;
                let id = v2::scr_id(start, &session_id);
                json!({"exists": false, "id": id, "path": v2::scr_path(start, &id),
                    "title": null, "title_by": null, "scope": scope, "previous": null,
                    "scribed_through": null, "updated": null, "sections": null,
                    "tag_entries": 0, "next_tag": "T1"})
            }
        },
    };
    let previous = v2::latest_in_scope(&v2_files, &scope, &session_id).map(|file| {
        json!({"id": file.front.id, "path": file.path, "title": file.front.title,
            "updated": file.front.updated, "session": file.front.session,
            "sections": if file.front.private() || withheld { Value::Null } else { sections(&file.sections) }})
    });
    let context_from = start.saturating_sub(CONTEXT_MESSAGES);
    let returned: Vec<Value> = if withheld {
        Vec::new()
    } else {
        messages
            .iter()
            .enumerate()
            .skip(context_from)
            .map(|(index, message)| {
                let mut value = json!(message);
                if index < start {
                    value["context"] = Value::Bool(true);
                }
                value
            })
            .collect()
    };
    let page = conversation::find_rules(&library.knowledge_pages()?)?;
    let rules = scribe::scribe_rules(page.as_ref(), project.as_deref());
    Ok(json!({
        "format": "v2",
        "session_id": session_id,
        "provider": provider,
        "parsed": parse_path,
        "content_withheld": withheld,
        "message_count": messages.len(),
        "new_count": messages.len().saturating_sub(start),
        "scr": record,
        "scope": scope,
        "previous": previous,
        "scribed_through": through.map(scribe::message_id),
        "messages": returned,
        "markers": marker_list(&user),
        "agent_markers": marker_list(&agent),
        "custom_title": if withheld { None } else { parsed.custom_title.clone() },
        "rules": rules,
        "tags": effective["tags"],
        "cadence": effective["cadence"],
    }))
}

fn sections(sections: &v2::Sections) -> Value {
    json!({"summary": sections.summary, "goals": sections.goals,
        "outcome": sections.outcome, "next": sections.next})
}

/// The v1 pass: the v1 SCR file, unchanged by v2.
fn v1(
    call: &Call<'_>,
    session_id: &str,
    provider: &str,
    session: &conversation::Session,
    messages: &[scribe::Message],
    parsed: Parsed,
) -> Result<Value> {
    let library = &call.ports.library;
    let session_id = session_id.to_owned();
    let provider = provider.to_owned();
    let withheld = session.sensitivity == "private";
    let project = parsed.project.clone();
    let messages = messages.to_vec();
    let files = scribe::scr_files(&library.scribe_files()?)?;
    let existing = scribe::find_session(&files, &session_id)?;
    let zone = call.ports.clock.time_zone();
    let scr = match existing {
        Some(file) => file.front.view(&file.path, true),
        None if messages.is_empty() => Value::Null,
        None => {
            let (id, path) = scribe::planned(&files, &parsed, &messages, None, zone.as_deref())?;
            json!({"exists":false,"id":id,"path":path,"title":null,"title_by":null,"scribed_through":null,
                "last_scribed_at":null,"topics":[],"aliases":[],"counterpart":null,"counterpart_name":null})
        }
    };
    let through = existing.and_then(|file| file.front.scribed_through.clone());
    let after = match &through {
        None => None,
        Some(id) => {
            let index = scribe::message_index(id).ok_or_else(|| Error::Library {
                code: "scribe_file_invalid",
                message: "the SCR file's scribed_through is not a message id".into(),
            })?;
            // Ids only grow with the transcript; a shorter parse is another file.
            if !withheld && index >= messages.len() {
                return Err(Error::Library {
                    code: "scribe_transcript_shrank",
                    message: "the transcript has fewer messages than scribed_through".into(),
                });
            }
            Some(index)
        }
    };
    // The tags in force and the cadence, from the Library settings. Read
    // before the parse is written, so an invalid config writes nothing.
    let scribe_settings = library.settings()?.scribe.effective();
    // A private session never reaches the scribe agent: nothing is written
    // or returned, as for a capture preview.
    let parse_path = if withheld {
        None
    } else {
        let meta = serde_json::to_string(&parsed).map_err(|_| Error::Library {
            code: "scribe_parse_invalid",
            message: "session facts could not be serialized".into(),
        })?;
        Some(library.write_scribe_parse(&session_id, &scribe::to_jsonl(&messages)?, &meta)?)
    };
    let start = after.map_or(0, |index| index + 1);
    let context_from = start.saturating_sub(CONTEXT_MESSAGES);
    let returned: Vec<Value> = messages
        .iter()
        .enumerate()
        .skip(context_from)
        .map(|(index, message)| {
            let mut value = json!(message);
            if index < start {
                value["context"] = Value::Bool(true);
            }
            value
        })
        .collect();
    let page = conversation::find_rules(&library.knowledge_pages()?)?;
    let rules = scribe::scribe_rules(page.as_ref(), project.as_deref());
    Ok(json!({
        "session_id": session_id,
        "provider": provider,
        "parsed": parse_path,
        "content_withheld": withheld,
        "message_count": messages.len(),
        "new_count": messages.len().saturating_sub(start),
        "scr": scr,
        "scribed_through": through,
        "messages": returned,
        "rules": rules,
        "tags": scribe_settings["tags"],
        "cadence": scribe_settings["cadence"],
    }))
}
