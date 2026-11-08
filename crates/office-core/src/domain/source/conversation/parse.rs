//! Claude Code JSONL transcript parsing.

use serde_json::Value;

use super::super::sha256_hex;
use super::paste::{closes_paste, is_side_session_brief, opens_paste, record_title};
use super::scrub::{scrub, strip_harness, tag_inner};
use super::types::{Line, Session, invalid};
use crate::Result;

/// Parse Claude Code JSONL. Unknown provider formats, mixed sessions, malformed
/// records and missing timestamps refuse without echoing untrusted input.
pub fn parse(raw: &str, expected_id: &str) -> Result<Session> {
    if expected_id.trim().is_empty()
        || expected_id.len() > 256
        || expected_id.chars().any(char::is_control)
    {
        return Err(invalid(
            "session_id must be a nonempty single-line id of at most 256 bytes",
        ));
    }
    let mut session = Session {
        orchestrator: "claude".into(),
        title: None,
        custom_title: None,
        model: None,
        project: None,
        branch: None,
        started_at: None,
        last_active_at: None,
        session_id: expected_id.into(),
        transcript_sha256: sha256_hex(raw.as_bytes()),
        scrubbed: scrub(raw) != raw,
        sensitivity: "normal".into(),
        lines: vec![],
        line_prefix_sha256: vec![],
    };
    let mut offset = 0usize;
    let mut first_user_seen = false;
    let mut custom_title_seen = false;
    for raw_line in raw.split_inclusive('\n') {
        offset += raw_line.len();
        if raw_line.trim().is_empty() {
            continue;
        }
        // The prefix ends at the record's content, excluding its line
        // separator, so appending later records cannot change it.
        let record_end = offset
            - usize::from(raw_line.ends_with('\n'))
            - usize::from(raw_line.ends_with("\r\n"));
        let record: Value = serde_json::from_str(raw_line)
            .map_err(|_| invalid("transcript must contain complete JSONL records"))?;
        let kind = record
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("transcript record needs a type"))?;
        // The latest `custom-title` record wins over every other title
        // record; any other title only fills a still-empty title.
        if kind == "custom-title" {
            if let Some(title) = record_title(&record, kind) {
                session.title = Some(title.clone());
                session.custom_title = Some(title);
                custom_title_seen = true;
            }
        } else if !custom_title_seen
            && session.title.is_none()
            && let Some(title) = record_title(&record, kind)
        {
            session.title = Some(title);
        }
        if !["user", "assistant"].contains(&kind) {
            continue;
        }
        if ["isMeta", "isSidechain", "isCompactSummary"]
            .iter()
            .any(|key| record.get(*key).and_then(Value::as_bool) == Some(true))
        {
            continue;
        }

        if record.get("sessionId").and_then(Value::as_str) != Some(expected_id) {
            return Err(invalid("transcript sessionId does not match session_id"));
        }
        let message = record
            .get("message")
            .ok_or_else(|| invalid("conversation record needs a message"))?;
        if message.get("role").and_then(Value::as_str) != Some(kind) {
            return Err(invalid("message role does not match record type"));
        }
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .filter(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
            .ok_or_else(|| invalid("conversation record needs an RFC 3339 timestamp"))?;
        if kind == "assistant"
            && let Some(model) = message
                .get("model")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty() && !m.starts_with('<'))
        {
            let clean = scrub(model);
            if clean != model || model.len() > 256 || model.chars().any(char::is_control) {
                return Err(invalid("invalid transcript model"));
            }
            session.model = Some(model.into());
        }
        let content = message
            .get("content")
            .ok_or_else(|| invalid("message needs content"))?;
        let texts: Vec<&str> = if let Some(text) = content.as_str() {
            vec![text]
        } else if let Some(blocks) = content.as_array() {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect()
        } else {
            return Err(invalid("message content must be text or content blocks"));
        };
        if session.started_at.is_none() {
            let context = |key: &str| {
                record
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            };
            let project = context("cwd");
            let branch = context("gitBranch");
            session.project = project.map(scrub);
            session.branch = branch.map(scrub);
            session.scrubbed |= project
                .is_some_and(|value| session.project.as_deref() != Some(value))
                || branch.is_some_and(|value| session.branch.as_deref() != Some(value));
            session.started_at = Some(timestamp.into());
        }
        if session.last_active_at.as_deref().is_none_or(|old| {
            chrono::DateTime::parse_from_rfc3339(old).ok()
                < chrono::DateTime::parse_from_rfc3339(timestamp).ok()
        }) {
            session.last_active_at = Some(timestamp.into());
        }
        let prefix_sha256 = sha256_hex(&raw.as_bytes()[..record_end]);
        // Scrub the entire text block, so multiline PEM credentials stay hidden.
        // For user records, drop harness wrappers (system reminders and
        // local-command output) before the line loop, so their interior
        // lines never reach a slug, the scribe agent or a verbatim quote.
        // A slash command renders as one `/name args` line in place of its
        // tags. Pasted content is the user's own words and is kept.
        let scrubbed_texts: Vec<String> = texts
            .iter()
            .map(|text| {
                let clean = scrub(text);
                session.scrubbed |= clean != *text;
                clean
            })
            .collect();
        // Extract the slash command before stripping, so a name and args
        // split across text blocks still render as one line.
        let mut command: Option<(usize, String)> = None;
        if kind == "user" {
            let name_at = scrubbed_texts
                .iter()
                .position(|text| tag_inner(text, "command-name").is_some());
            if let Some(at) = name_at
                && let Some(raw_name) = tag_inner(&scrubbed_texts[at], "command-name")
            {
                let name = raw_name.trim().trim_start_matches('/');
                if !name.is_empty() {
                    let args = scrubbed_texts
                        .iter()
                        .find_map(|text| tag_inner(text, "command-args"))
                        .unwrap_or_default();
                    let args = args.trim();
                    command = Some((
                        at,
                        if args.is_empty() {
                            format!("/{name}")
                        } else {
                            format!("/{name} {args}")
                        },
                    ));
                }
            }
        }
        let mut cleaned: Vec<String> = scrubbed_texts
            .iter()
            .map(|text| {
                if kind == "user" {
                    strip_harness(text)
                } else {
                    text.clone()
                }
            })
            .collect();
        if let Some((at, command)) = command {
            cleaned.insert(at.min(cleaned.len()), command);
        }
        // A desktop side session opens with an orchestrator-written brief
        // as its first user record. It is the orchestrator's text, not
        // Daniel's: keep its message (and id) but as an assistant message,
        // so it never takes Daniel's tags and never names the SCR file.
        let effective_role = if kind == "user" && !first_user_seen {
            first_user_seen = true;
            let joined = cleaned.join("\n");
            if is_side_session_brief(&joined) {
                "assistant"
            } else {
                kind
            }
        } else {
            kind
        };
        // A paste wrapper can span blocks of one record's text, but never
        // records: each record starts outside one.
        let mut pasted = false;
        for text in &cleaned {
            let mut fenced = false;
            for text in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let in_paste = if kind == "user" {
                    let opening = !pasted && opens_paste(text);
                    let inside = pasted || opening;
                    pasted = inside && !closes_paste(text);
                    inside
                } else {
                    false
                };
                if text.starts_with("```") || text.starts_with("~~~") {
                    fenced = !fenced;
                    continue;
                }
                if fenced || text.starts_with('>') {
                    continue;
                }
                session.lines.push(Line {
                    line: session.lines.len(),
                    role: effective_role.into(),
                    timestamp: timestamp.into(),
                    text: text.into(),
                    context: false,
                    pasted: in_paste,
                });
                session.line_prefix_sha256.push(prefix_sha256.clone());
            }
        }
    }
    if session.lines.is_empty() {
        return Err(invalid(
            "no supported Claude Code conversation text was found",
        ));
    }
    // Session ids are provenance, never a channel for a credential.
    if scrub(expected_id) != expected_id {
        return Err(invalid("invalid transcript session id"));
    }
    Ok(session)
}
