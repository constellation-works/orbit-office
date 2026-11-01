//! Session transcript adapters, evidence selection and source provenance.
use super::{format_conversation_entry, sha256_hex};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Bounded raw transcript input (8 MiB); no raw transcript is persisted.
pub const INPUT_LIMIT: u64 = 8 * 1024 * 1024;
fn is_false(value: &bool) -> bool {
    !*value
}

/// One scrubbed, numbered text line; tool results and thinking are excluded.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    /// Zero-based position in the adapter output, used by model selections.
    pub line: usize,
    /// Transcript speaker, never caller-supplied authority.
    pub role: String,
    /// Original message timestamp.
    pub timestamp: String,
    /// Exact line after credential scrubbing and outer whitespace removal.
    pub text: String,
    /// Read-only context line in an incremental preview; never selectable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub context: bool,
}
/// Parsed Claude Code session; future providers implement this same shape.
#[derive(Clone, Debug, Serialize)]
pub struct Session {
    /// Provider inferred from the adapter's record format.
    pub orchestrator: String,
    /// Last non-synthetic model reported by an assistant message.
    pub model: Option<String>,
    /// Checked against every conversational record's sessionId.
    pub session_id: String,
    /// Digest of the raw input bytes, used only for idempotency.
    pub transcript_sha256: String,
    /// True when any credential was removed.
    pub scrubbed: bool,
    /// Strictest configured, existing-source and credential-derived privacy.
    pub sensitivity: String,
    /// Text only, with original timestamps and roles.
    pub lines: Vec<Line>,
    /// SHA-256 of the raw transcript bytes up to each line's source record,
    /// parallel to `lines`. Lets a later pass prove the transcript only grew.
    #[serde(skip)]
    pub line_prefix_sha256: Vec<String>,
}
/// A cheap model selects evidence by number, never supplies replacement text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Zero-based line in the preview.
    pub line: usize,
    /// decision, direction, fact, action or open; no handoff or resolved.
    pub kind: String,
}
/// Transcript capture request, separate from upload and WebVTT capture.
pub struct Request {
    /// Contained UTF-8 JSONL input path.
    pub path: String,
    /// Expected transcript session id.
    pub session_id: String,
    /// Optional model evidence choices; absent uses conservative classification.
    pub selections: Option<Vec<Selection>>,
    /// Preview digest; required whenever selections are supplied.
    pub expected_sha256: Option<String>,
    /// First line this pass considers (zero-based, inclusive); earlier lines
    /// are read-only context and selecting one refuses the write.
    pub from_line: Option<usize>,
}
fn invalid(message: &'static str) -> Error {
    Error::Library {
        code: "invalid_session_transcript",
        message: message.into(),
    }
}

/// Remove high-confidence credential shapes and secret assignment values.
/// Operates before previews as well as before persistence; does not read env.
pub fn scrub(text: &str) -> String {
    let mut out = String::new();
    let mut pem = false;
    let mut next_secret = false;
    for part in text.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
        let space = &part[word.len()..];
        if word.contains("-----BEGIN") {
            pem = true;
        }
        let lower = word.to_ascii_lowercase();
        let prefix = [
            "sk-",
            "ghp_",
            "gho_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "AKIA",
            "eyJ",
        ]
        .iter()
        .any(|prefix| {
            word.match_indices(prefix).any(|(at, _)| {
                let boundary = word[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
                let credential = word[at + prefix.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || "_-./+=".contains(*c))
                    .count();
                boundary && credential >= 16
            })
        });
        let assignment = word.split_once('=').or_else(|| word.split_once(':'));
        let secret_assignment = assignment.is_some_and(|(key, value)| {
            let key = key
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .to_ascii_lowercase();
            [
                "token",
                "password",
                "secret",
                "api_key",
                "apikey",
                "access_token",
            ]
            .contains(&key.as_str())
                && value.len() >= 8
        });
        let url_credentials = url::Url::parse(word).is_ok_and(|url| url.password().is_some());
        if pem || prefix || next_secret || secret_assignment || url_credentials {
            out.push_str("[REDACTED]");
        } else {
            out.push_str(word);
        }
        out.push_str(space);
        next_secret = lower.trim_matches(['"', '\'', ':']) == "bearer"
            || [
                "token=",
                "password=",
                "secret=",
                "api_key=",
                "token:",
                "password:",
                "secret:",
                "api_key:",
            ]
            .contains(&lower.as_str());
        if word.contains("-----END") {
            pem = false;
        }
    }
    out
}

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
        model: None,
        session_id: expected_id.into(),
        transcript_sha256: sha256_hex(raw.as_bytes()),
        scrubbed: scrub(raw) != raw,
        sensitivity: "normal".into(),
        lines: vec![],
        line_prefix_sha256: vec![],
    };
    let mut offset = 0usize;
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
        let prefix_sha256 = sha256_hex(&raw.as_bytes()[..record_end]);
        // Scrub the entire text block, so multiline PEM credentials stay hidden.
        for text in texts {
            let clean = scrub(text);
            session.scrubbed |= clean != text;
            let mut fenced = false;
            for text in clean.lines().map(str::trim).filter(|line| !line.is_empty()) {
                if text.starts_with("```") || text.starts_with("~~~") {
                    fenced = !fenced;
                    continue;
                }
                if fenced || text.starts_with('>') {
                    continue;
                }
                session.lines.push(Line {
                    line: session.lines.len(),
                    role: kind.into(),
                    timestamp: timestamp.into(),
                    text: text.into(),
                    context: false,
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
fn ambiguous(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains('?')
        || [
            "maybe", "perhaps", "thinking", "might", "could", "not sure", "if ", "consider",
        ]
        .iter()
        .any(|s| lower.contains(s))
}
const DECISION_CUES: &[&str] = &["decision:", "i decided ", "we decided ", "i choose "];
const DIRECTION_CUES: &[&str] = &[
    "direction:",
    "always ",
    "never ",
    "please ",
    "we need to ",
    "i want you to ",
];

/// Text after a leading `Note:`/`Scribe:` command prefix, if this is one.
fn note_remainder(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    ["note:", "scribe:"]
        .iter()
        .find(|prefix| lower.starts_with(*prefix))
        .map(|prefix| text[prefix.len()..].trim_start())
}

/// Deterministic kind for a `Note:`/`Scribe:` command: a decision or
/// direction cue in the text after the prefix wins, otherwise a fact.
/// Commands are captured without model judgment, so ambiguity in the
/// remainder does not demote them the way ordinary lines are demoted.
pub fn note_kind(text: &str) -> Option<&'static str> {
    let rest = note_remainder(text)?;
    let lower = rest.to_ascii_lowercase();
    if DECISION_CUES.iter().any(|p| lower.starts_with(p)) {
        Some("decision")
    } else if DIRECTION_CUES.iter().any(|p| lower.starts_with(p)) {
        Some("direction")
    } else {
        Some("fact")
    }
}

fn explicit_kind(line: &Line) -> &'static str {
    let lower = line.text.to_ascii_lowercase();
    if line.role == "user"
        && let Some(kind) = note_kind(&line.text)
    {
        return kind;
    }
    if ambiguous(&line.text) {
        return "open";
    }
    if line.role == "assistant" {
        return if lower.starts_with("action:") {
            "action"
        } else {
            "open"
        };
    }
    if DECISION_CUES.iter().any(|p| lower.starts_with(p)) {
        "decision"
    } else if DIRECTION_CUES.iter().any(|p| lower.starts_with(p)) {
        "direction"
    } else if lower.starts_with("fact:") {
        "fact"
    } else {
        "open"
    }
}
/// Validate model selections and render exact evidence lines. An unclear or
/// non-user decision/direction becomes open even if a model misclassifies it.
pub fn entries(session: &Session, request: &Request) -> Result<Vec<String>> {
    if request.selections.is_some()
        && request.expected_sha256.as_deref() != Some(&session.transcript_sha256)
    {
        return Err(invalid(
            "transcript changed since preview; read a fresh preview",
        ));
    }
    if let Some(from_line) = request.from_line
        && let Some(selections) = &request.selections
        && selections.iter().any(|s| s.line < from_line)
    {
        return Err(invalid(
            "selections before from_line name read-only context and cannot be selected",
        ));
    }
    // `Note:`/`Scribe:` commands are captured without model judgment: add
    // any the crew did not select, in the range this pass considers.
    let from_line = request.from_line.unwrap_or(0);
    let augmented;
    let defaults;
    let selections = match &request.selections {
        Some(selections) => {
            let chosen: std::collections::BTreeSet<usize> =
                selections.iter().map(|s| s.line).collect();
            let mut all = selections.clone();
            for line in &session.lines {
                if line.line >= from_line
                    && line.role == "user"
                    && !chosen.contains(&line.line)
                    && let Some(kind) = note_kind(&line.text)
                {
                    all.push(Selection {
                        line: line.line,
                        kind: kind.into(),
                    });
                }
            }
            augmented = all;
            &augmented
        }
        None => {
            defaults = session
                .lines
                .iter()
                .filter(|l| l.role == "user" || l.text.to_ascii_lowercase().starts_with("action:"))
                .map(|l| Selection {
                    line: l.line,
                    kind: explicit_kind(l).into(),
                })
                .collect::<Vec<_>>();
            &defaults
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    for selection in selections {
        if !["decision", "direction", "action", "fact", "open"].contains(&selection.kind.as_str())
            || !seen.insert(selection.line)
        {
            return Err(invalid(
                "selections need unique lines and a supported extraction kind",
            ));
        }
        let line = session
            .lines
            .get(selection.line)
            .ok_or_else(|| invalid("selection line is outside the transcript"))?;
        let mut kind = selection.kind.as_str();
        // A command's kind is fixed by its cue, not by the crew's choice.
        let forced = (line.role == "user")
            .then(|| note_kind(&line.text))
            .flatten();
        if let Some(forced) = forced {
            kind = forced;
        } else if (["decision", "direction"].contains(&kind) && explicit_kind(line) != kind)
            || (kind == "fact" && (line.role != "user" || ambiguous(&line.text)))
            || (kind == "action" && line.role != "assistant")
        {
            kind = "open";
        }
        ordered.push((
            selection.line,
            format_conversation_entry(&line.timestamp, kind, &line.text)?,
        ));
    }
    ordered.sort_by_key(|(line, _)| *line);
    Ok(ordered.into_iter().map(|(_, text)| text).collect())
}
/// Validate nullable session provenance on old and new sources.
pub fn validate_provenance(markdown: &str) -> Result<()> {
    let fields = crate::domain::metrics::metadata(markdown)?;
    for key in ["orchestrator", "model", "session_id"] {
        if let Some(value) = fields.get(key)
            && !value.is_null()
            && !value.as_str().is_some_and(|s| {
                !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidFrontmatter {
                field: key.into(),
                reason: "expected a nonempty single-line string or null",
            });
        }
    }
    Ok(())
}
/// Add transcript provenance without widening the general capture request.
fn provenance(session: &Session) -> Value {
    json!({"orchestrator":session.orchestrator,"model":session.model,"session_id":session.session_id,"transcript_sha256":session.transcript_sha256})
}

/// Scrub copied source text too: a handoff appended since the last extraction
/// must not introduce credentials into a newly persisted historical snapshot.
/// Clean source bytes stay identical, including their existing formatting.
pub fn scrub_source(markdown: &str) -> Result<String> {
    use crate::domain::knowledge::frontmatter::flow_value;
    fn clean(value: &Value) -> Value {
        match value {
            Value::String(text) => json!(scrub(text)),
            Value::Array(items) => Value::Array(items.iter().map(clean).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), clean(value)))
                    .collect(),
            ),
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }
    let fields = crate::domain::metrics::metadata(markdown)?;
    let old_body = super::source_body(markdown);
    let body = scrub(&old_body);
    let cleaned = fields
        .iter()
        .map(|(key, value)| (key.clone(), clean(value)))
        .collect::<serde_json::Map<_, _>>();
    if cleaned == fields && body == old_body {
        return Ok(markdown.into());
    }
    let mut result = String::from("---\n");
    for (key, value) in cleaned {
        result.push_str(&format!("{key}: {}\n", flow_value(&value)));
    }
    result.push_str("---\n");
    result.push_str(&body);
    Ok(result)
}

/// Recorded extraction progress of a conversation source, if it has run a
/// pass under the incremental scheme. Legacy sources return `(None, None)`.
pub fn extracted_state(markdown: &str) -> Result<(Option<usize>, Option<String>)> {
    let fields = crate::domain::metrics::metadata(markdown)?;
    let through = fields.get("extracted_through").and_then(|value| {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    });
    let prefix = fields
        .get("extracted_prefix_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((through, prefix))
}

/// True when this session's bytes up to the source's recorded
/// `extracted_through` hash to its `extracted_prefix_sha256`: the
/// transcript only grew since that pass, so new entries may append.
pub fn prefix_grew(session: &Session, markdown: &str) -> Result<bool> {
    let (through, prefix) = extracted_state(markdown)?;
    Ok(match (through, prefix) {
        (Some(through), Some(prefix)) => session
            .line_prefix_sha256
            .get(through)
            .is_some_and(|hash| hash == &prefix),
        _ => false,
    })
}

/// Merge new evidence append-only, retaining handoffs and old citation anchors.
/// The caller journals the returned Markdown with an immutable prior snapshot.
pub fn merge(
    markdown: &str,
    session: &Session,
    new_entries: &[String],
    sensitivity: &str,
    changed: bool,
) -> Result<String> {
    use crate::domain::knowledge::frontmatter::flow_value;
    let mut fields = crate::domain::metrics::metadata(markdown)?;
    let version = super::parse_source_meta(markdown).version;
    let next = if changed {
        version
            .checked_add(1)
            .ok_or_else(|| invalid("conversation version overflow"))?
    } else {
        version
    };
    for (key, value) in provenance(session)
        .as_object()
        .ok_or_else(|| invalid("invalid provenance"))?
    {
        fields.insert(key.clone(), value.clone());
    }
    fields.insert("sensitivity".into(), json!(sensitivity));
    fields.insert("version".into(), json!(next));
    fields.insert("captured_by".into(), json!("transcript-extractor"));
    // Every successful pass advances the incremental cursor, even one that
    // selected nothing; the next pass starts after the last line here.
    if let (Some(last), Some(prefix)) = (session.lines.last(), session.line_prefix_sha256.last()) {
        fields.insert("extracted_through".into(), json!(last.line));
        fields.insert("extracted_prefix_sha256".into(), json!(prefix));
    }
    // Any new evidence revokes previous sharing approval.
    fields.insert("shareable".into(), json!(false));
    if changed {
        fields.insert("supersedes".into(), json!(format!("v{version}.md")));
    }
    let mut body = super::source_body(markdown);
    let mut known: std::collections::BTreeSet<String> = body.lines().map(str::to_owned).collect();
    for entry in new_entries {
        if known.insert(entry.clone()) {
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(entry);
            body.push('\n');
        }
    }
    let mut result = String::from("---\n");
    for (key, value) in fields {
        result.push_str(&format!("{key}: {}\n", flow_value(&value)));
    }
    result.push_str("---\n");
    result.push_str(&body);
    validate_provenance(&result)?;
    Ok(result)
}
