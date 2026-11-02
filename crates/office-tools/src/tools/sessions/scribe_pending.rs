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

use office_core::domain::scribe_control::validate_session_id;
use office_core::domain::source::conversation::{
    self,
    scribe::{self, CONTEXT_MESSAGES, PROVIDER, Parsed},
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
    let parsed = Parsed {
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
    };
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
