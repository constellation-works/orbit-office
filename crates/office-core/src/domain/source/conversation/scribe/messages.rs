//! Parsed message model, stable ids and JSONL representation.

use serde::{Deserialize, Serialize};

use super::super::Session;
use super::{PARSE_DIR, invalid, refusal};
use crate::Result;

/// One message exchanged between the user and the agent, as the scribe
/// reads it: the kept text of one transcript record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// Stable id: the nth kept message is always `m-<n>` (from `m-0001`).
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    /// The record's RFC 3339 timestamp.
    pub at: String,
    /// The record's kept, scrubbed text lines joined by newlines.
    pub text: String,
    /// 1-based lines of `text` inside a paste region the adapter saw; a
    /// marker there is never one. Absent when the message has none, so
    /// earlier message files still read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pasted: Vec<usize>,
}

/// The id of the message at zero-based `index`.
#[must_use]
pub fn message_id(index: usize) -> String {
    format!("m-{:04}", index.saturating_add(1))
}

/// The zero-based index a canonical message id names.
#[must_use]
pub fn message_index(id: &str) -> Option<usize> {
    let digits = id.strip_prefix("m-")?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let index = digits.parse::<usize>().ok()?.checked_sub(1)?;
    (message_id(index) == id).then_some(index)
}

/// Group the adapter's text lines into messages. Lines of one transcript
/// record share a prefix digest, so appending records only adds messages.
#[must_use]
pub fn messages(session: &Session) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut last: Option<&str> = None;
    for (line, prefix) in session.lines.iter().zip(&session.line_prefix_sha256) {
        match out.last_mut() {
            Some(message) if last == Some(prefix.as_str()) => {
                message.text.push('\n');
                message.text.push_str(&line.text);
                if line.pasted {
                    let number = message.text.lines().count();
                    message.pasted.push(number);
                }
            }
            _ => out.push(Message {
                id: message_id(out.len()),
                role: line.role.clone(),
                at: line.timestamp.clone(),
                text: line.text.clone(),
                pasted: if line.pasted { vec![1] } else { vec![] },
            }),
        }
        last = Some(prefix);
    }
    out
}

/// One JSON object per line, in id order.
pub fn to_jsonl(messages: &[Message]) -> Result<String> {
    let mut out = String::new();
    for message in messages {
        out.push_str(
            &serde_json::to_string(message).map_err(|_| invalid("unserializable message"))?,
        );
        out.push('\n');
    }
    Ok(out)
}

/// Read a parsed message file; ids must run `m-0001` upward.
pub fn from_jsonl(text: &str) -> Result<Vec<Message>> {
    let damaged = || {
        refusal(
            "scribe_parse_invalid",
            "the parsed message file is damaged; run office.scribe-pending again",
        )
    };
    let mut out = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let message: Message = serde_json::from_str(line).map_err(|_| damaged())?;
        if message.id != message_id(out.len()) || !["user", "assistant"].contains(&&*message.role) {
            return Err(damaged());
        }
        out.push(message);
    }
    Ok(out)
}

/// Session facts `office.scribe-pending` records beside the message file,
/// so `office.scribe-append` can name and describe the SCR file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parsed {
    /// Provider session id.
    pub session_id: String,
    /// Transcript format, `claude-code`.
    pub provider: String,
    /// Last model the transcript reports.
    pub model: Option<String>,
    /// Host the session was parsed on.
    pub host: Option<String>,
    /// Project name: the last component of `cwd`.
    pub project: Option<String>,
    /// The session's working directory.
    pub cwd: Option<String>,
    /// Git branch at session start.
    pub branch: Option<String>,
    /// Transcript path as given.
    pub transcript: String,
    /// Workspace-relative path of the message file.
    pub parsed: String,
    /// First conversational record's timestamp.
    pub started_at: Option<String>,
    /// Title from the transcript's title/summary record, if any. Only a
    /// side session (one opening with an orchestrator brief) names its
    /// SCR file from it; see [`super::planned`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_title: Option<String>,
    /// Latest `customTitle`, from the transcript's `custom-title`
    /// records or the caller. A set custom title names the SCR file
    /// for every session; see [`super::planned`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    /// Scribe v2: the session scope token (`global`, `project:<name>`)
    /// `office.scribe-pending` resolved; absent on a v1 parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Scribe v2: the session is private. Its parse is written for the
    /// local, reference-only pass; the v1 append refuses it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
}

/// Workspace-relative path of a session's message file, given the
/// workspace-relative, `/`-separated Library directory.
#[must_use]
pub fn parse_path(library_dir: &str, session_id: &str) -> String {
    let library_dir = library_dir.trim_end_matches('/');
    if library_dir.is_empty() {
        format!("{PARSE_DIR}/{session_id}.jsonl")
    } else {
        format!("{library_dir}/{PARSE_DIR}/{session_id}.jsonl")
    }
}

/// The project name of a working directory. A Claude Code worktree
/// cwd (`<repo>/.claude/worktrees/<name>`) names its repository, even
/// when that worktree directory no longer exists; any other directory
/// names itself (its last path component).
#[must_use]
pub fn project_of(cwd: &str) -> Option<String> {
    let parts: Vec<&str> = cwd
        .trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() >= 4
        && parts[parts.len() - 2] == "worktrees"
        && parts[parts.len() - 3] == ".claude"
    {
        return Some(parts[parts.len() - 4].to_owned()).filter(|name| !name.is_empty());
    }
    parts.last().map(|name| (*name).to_owned())
}
