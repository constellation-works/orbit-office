//! Session transcript adapters, evidence selection and source provenance.

/// Append-only Scribe corrections and user-attributed entries.
pub mod corrections;
/// Stored reference extraction and model-free recall selection.
pub mod recall;
/// Scribe v1: parsed messages, rules sections and SCR files.
pub mod scribe;
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
/// First line of the brief a desktop side session opens with: the
/// orchestrator that started the session wrote it, not Daniel. The
/// desktop app writes it parenthesised, with a trailing colon
/// (`Background (from the session that started this one):`); the
/// unparenthesised form occurs too.
pub const SIDE_SESSION_BRIEF_PREFIX: &str = "Background from the session that started this one";

/// True when `text` opens with the side-session brief preamble. Both
/// written forms match, case-insensitively, ignoring the parentheses
/// and a trailing colon on the preamble.
#[must_use]
pub fn is_side_session_brief(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    let plain: String = lower.chars().filter(|c| *c != '(' && *c != ')').collect();
    let plain = plain.trim_start();
    let Some(rest) = plain.strip_prefix("background from the session that started this one") else {
        return false;
    };
    rest.chars()
        .next()
        .is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// The brief's first prose sentence after the preamble line, skipping
/// any `Task:`/`Title:` line and blank lines. The sentence ends at its
/// terminating `.`, `!` or `?`, or at the end of its paragraph.
#[must_use]
pub fn brief_first_sentence(text: &str) -> Option<String> {
    if !is_side_session_brief(text) {
        return None;
    }
    let body = text.lines().skip(1).collect::<Vec<_>>().join("\n");
    for paragraph in body.split("\n\n") {
        let prose: Vec<&str> = paragraph
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && !line.to_ascii_lowercase().starts_with("task:")
                    && !line.to_ascii_lowercase().starts_with("title:")
            })
            .collect();
        if prose.is_empty() {
            continue;
        }
        let joined = prose.join(" ");
        let end = joined
            .char_indices()
            .find(|(_, c)| ['.', '!', '?'].contains(c))
            .map_or(joined.len(), |(at, c)| at + c.len_utf8());
        let sentence = joined[..end]
            .trim()
            .trim_end_matches(['.', '!', '?'])
            .trim();
        if !sentence.is_empty() {
            return Some(sentence.to_owned());
        }
    }
    None
}

/// The brief's task-title line: a `Task:`/`Title:` line (optionally
/// Markdown-bold) after the preamble, if the brief carries one.
#[must_use]
pub fn brief_task_title(text: &str) -> Option<String> {
    if !is_side_session_brief(text) {
        return None;
    }
    for line in text.lines().skip(1) {
        let plain = line.trim().trim_start_matches('#').trim();
        let plain = plain.strip_prefix("**").unwrap_or(plain);
        let lower = plain.to_ascii_lowercase();
        for label in ["task:", "title:"] {
            if let Some(rest) = lower.strip_prefix(label) {
                let at = plain.len() - rest.len();
                let value = plain[at..]
                    .trim()
                    .trim_end_matches("**")
                    .trim()
                    .trim_matches('"');
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

/// A title carried by a non-conversational record, if any: a `summary`
/// record's summary, or an `aiTitle`/`customTitle`/`title` field. Values
/// over 256 bytes are dropped, as staging drops them.
fn record_title(record: &Value, kind: &str) -> Option<String> {
    let keys: &[&str] = if kind == "summary" {
        &["summary", "title", "aiTitle", "customTitle"]
    } else if kind == "custom-title" {
        &["customTitle", "aiTitle", "title", "summary"]
    } else if kind.contains("title") {
        &["aiTitle", "customTitle", "title", "summary"]
    } else {
        &["aiTitle", "customTitle"]
    };
    keys.iter().find_map(|key| {
        record
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| {
                !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
            })
            .map(scrub)
    })
}

/// Parsed Claude Code session; future providers implement this same shape.
#[derive(Clone, Debug, Serialize)]
pub struct Session {
    /// Provider inferred from the adapter's record format.
    pub orchestrator: String,
    /// Title from the transcript's title/summary record, if any.
    pub title: Option<String>,
    /// Latest `custom-title` record's `customTitle`, if any.
    pub custom_title: Option<String>,
    /// Last non-synthetic model reported by an assistant message.
    pub model: Option<String>,
    /// Working directory and branch of the first conversational record.
    pub project: Option<String>,
    /// Git branch at session start, absent outside a repository.
    pub branch: Option<String>,
    /// First supported conversational record timestamp.
    pub started_at: Option<String>,
    /// Latest supported conversational record timestamp.
    pub last_active_at: Option<String>,
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
    /// Zero-based assistant question line a user reply answers. Lets a
    /// short reply become a `decision` only when the answer checks pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<usize>,
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
    /// Crew-written scribe handoff, present only when the rules page
    /// enables it; written by [`append_handoff`] or dropped.
    pub handoff: Option<String>,
    /// Hook context and crew/user title; never used as authority.
    pub context: recall::Context,
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

/// The inner text of the first `<tag>…</tag>` block in `text`, if any.
fn tag_inner(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = text.find(&open)?;
    let after = &text[start + open.len()..];
    if !after.starts_with('>') && !after.starts_with(char::is_whitespace) {
        return None;
    }
    let content_start = start + text[start..].find('>')? + 1;
    let close = format!("</{tag}>");
    let end = text[content_start..].find(&close)? + content_start;
    Some(text[content_start..end].trim().to_owned())
}

/// Remove every `<tag>…</tag>` block (and any stray tag) from `text`.
/// An unclosed block removes the rest of the text: harness blocks are
/// always closed, so a missing close means truncated wrapper text.
fn strip_tag_blocks(text: &str, tag: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let open = format!("<{tag}");
        let Some(start) = out.find(&open) else {
            break;
        };
        let after = &out[start + open.len()..];
        if !after.starts_with('>') && !after.starts_with(char::is_whitespace) {
            break;
        }
        let Some(gt) = out[start..].find('>') else {
            out.truncate(start);
            break;
        };
        let content_start = start + gt + 1;
        let close = format!("</{tag}>");
        match out[content_start..].find(&close) {
            Some(at) => out.replace_range(start..content_start + at + close.len(), ""),
            None => out.truncate(start),
        }
    }
    out.replace(&format!("</{tag}>"), "")
}

/// Remove every block whose tag starts with `prefix`, such as
/// `<local-command-stdout>` and `<local-command-stderr>`.
fn strip_prefixed_blocks(text: &str, prefix: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let open = format!("<{prefix}");
        let Some(start) = out.find(&open) else {
            break;
        };
        let Some(gt) = out[start..].find('>') else {
            out.truncate(start);
            break;
        };
        let tag: String = out[start + 1..start + gt]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        let content_start = start + gt + 1;
        let close = format!("</{tag}>");
        match out[content_start..].find(&close) {
            Some(at) => out.replace_range(start..content_start + at + close.len(), ""),
            None => out.truncate(start),
        }
    }
    out
}

/// Strip harness wrapper blocks from one user text, leaving only the
/// words the user typed. Slash-command tags are removed here too; the
/// caller renders the invocation from the extracted name and args.
fn strip_harness(text: &str) -> String {
    let out = strip_tag_blocks(text, "system-reminder");
    let out = strip_tag_blocks(&out, "task-notification");
    let out = strip_prefixed_blocks(&out, "local-command-");
    let out = strip_tag_blocks(&out, "command-message");
    let out = strip_tag_blocks(&out, "command-name");
    strip_tag_blocks(&out, "command-args")
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
        for text in &cleaned {
            let mut fenced = false;
            for text in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
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
/// Replies that accept a yes/no question, matched as a leading phrase.
const AFFIRMATIVES: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "do it",
    "go ahead",
    "go for it",
    "sounds good",
    "ship it",
    "please do",
    "lgtm",
    "agreed",
    "approved",
];
/// Replies that decline a yes/no question, matched as a leading phrase.
const NEGATIVES: &[&str] = &[
    "no",
    "nope",
    "not now",
    "not yet",
    "no thanks",
    "don't",
    "do not",
];
/// Words that qualify a reply, so it no longer simply answers the question.
const QUALIFIERS: &[&str] = &["but", "except", "unless", "instead", "wait"];
/// Words that turn an affirmative into something else.
const NEGATIONS: &[&str] = &["no", "not", "nope", "don't", "dont", "never"];
/// Question words that make a question open rather than yes/no.
const WH_WORDS: &[&str] = &[
    "what", "which", "where", "when", "who", "whom", "whose", "why", "how",
];

fn normalized(text: &str) -> String {
    text.trim().to_lowercase().replace('\u{2019}', "'")
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect()
}

/// True when `text` begins with `phrase` followed by a non-word character.
fn leads_with(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '\''))
    })
}

/// Option labels a question offers: `(a)`, `a)`, `(1)`, `1)` or `option a`.
fn option_labels(question: &str) -> Vec<String> {
    let lower = normalized(question);
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let mut labels = Vec::new();
    let short = |label: &str| {
        (1..=2).contains(&label.len()) && label.chars().all(|c| c.is_ascii_alphanumeric())
    };
    for (index, token) in tokens.iter().enumerate() {
        let token = token.trim_end_matches([',', ';', ':', '.', '?']);
        let bare = token.strip_prefix('(').unwrap_or(token);
        if let Some(label) = bare.strip_suffix(')')
            && short(label)
        {
            labels.push(label.to_string());
        }
        if (token == "option" || token == "options")
            && let Some(next) = tokens.get(index + 1)
        {
            let label = next.trim_end_matches([',', ';', ':', '.', '?']);
            if short(label) {
                labels.push(label.to_string());
            }
        }
    }
    labels.sort();
    labels.dedup();
    labels
}

/// The rendered answer text when `reply` answers `question` under the
/// deterministic checks (specs/sessions.md, Answers to questions):
/// `question` is the nearest assistant line before the reply with no user
/// line between them and ends in `?`; the reply carries no uncertainty
/// marker and is not a question; it is an affirmative or negative to a
/// yes/no question, or an option label the question offered.
fn answer(session: &Session, reply: usize, question: usize) -> Option<String> {
    let asked = session.lines.get(question)?;
    let said = session.lines.get(reply)?;
    // Lines are numbered by position, so the nearest earlier assistant line
    // with no user line in between is exactly the previous line.
    // Private sessions stay on the local conservative path, which never
    // has crew answers.
    if session.sensitivity == "private"
        || question.checked_add(1) != Some(reply)
        || asked.role != "assistant"
        || said.role != "user"
        || !asked.text.trim_end().ends_with('?')
        || ambiguous(&said.text)
    {
        return None;
    }
    let reply_text = normalized(&said.text);
    let reply_words = words(&reply_text);
    if reply_words.iter().any(|word| QUALIFIERS.contains(word)) {
        return None;
    }
    let labels = option_labels(&asked.text);
    let verb = if labels.is_empty() {
        let question_text = normalized(&asked.text);
        let question_words = words(&question_text);
        let yes_no = question_words
            .first()
            .is_some_and(|first| !WH_WORDS.contains(first))
            && !question_words.contains(&"or");
        if !yes_no {
            return None;
        }
        if AFFIRMATIVES.iter().any(|p| leads_with(&reply_text, p))
            && !reply_words.iter().any(|word| NEGATIONS.contains(word))
        {
            "answering"
        } else if NEGATIVES.iter().any(|p| leads_with(&reply_text, p)) {
            "declining"
        } else {
            return None;
        }
    } else {
        let choice = reply_text.trim_end_matches(['.', '!']);
        let choice = choice.strip_prefix("option ").unwrap_or(choice);
        let choice = choice.strip_prefix('(').unwrap_or(choice);
        let choice = choice.strip_suffix(')').unwrap_or(choice);
        if !labels.iter().any(|label| label == choice) {
            return None;
        }
        "answering"
    };
    Some(format!(
        "\"{}\" ({verb} #L{question}: \"{}\")",
        said.text, asked.text
    ))
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
                        answers: None,
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
                    answers: None,
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
        let mut text = line.text.clone();
        // A command's kind is fixed by its cue, not by the crew's choice.
        let forced = (line.role == "user")
            .then(|| note_kind(&line.text))
            .flatten();
        if let Some(forced) = forced {
            kind = forced;
        } else if kind == "decision"
            && explicit_kind(line) != kind
            && let Some(answer) = selection
                .answers
                .and_then(|question| answer(session, selection.line, question))
        {
            text = answer;
        } else if (["decision", "direction"].contains(&kind) && explicit_kind(line) != kind)
            || (kind == "fact" && (line.role != "user" || ambiguous(&line.text)))
            || (kind == "action" && line.role != "assistant")
        {
            kind = "open";
        }
        ordered.push((
            selection.line,
            format_conversation_entry(&line.timestamp, kind, &text)?,
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
    if fields.get("kind").and_then(Value::as_str) == Some("conversation") {
        recall::validate_context(&fields)?;
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

/// Library path of the scribe rules page (`dom-scribe-rules`).
pub const RULES_PAGE_PATH: &str = "knowledge/domain/dom-scribe-rules.md";
/// Word limit of a scribe handoff.
pub const HANDOFF_WORD_LIMIT: usize = 60;

/// The scribe rules page as the crew and the adapter see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rules {
    /// Always `dom-scribe-rules`.
    pub id: String,
    /// Library-relative path.
    pub path: String,
    /// `scribe.handoff`; false when absent.
    pub handoff: bool,
    /// The page body, passed to the crew as data; `None` for a private page,
    /// which never reaches the crew.
    pub body: Option<String>,
}

/// Parse the rules page. Rules only steer the crew: nothing here changes
/// the deterministic guard in [`entries`].
pub fn rules(markdown: &str) -> Result<Rules> {
    let page = crate::domain::knowledge::Page::from_markdown(markdown)?;
    if page.id != crate::domain::knowledge::SCRIBE_RULES_PAGE {
        return Err(Error::InvalidFrontmatter {
            field: "id".into(),
            reason: "the scribe rules page has the id dom-scribe-rules",
        });
    }
    let private = page.sensitivity == office_common::Sensitivity::Private;
    Ok(Rules {
        id: page.id,
        path: RULES_PAGE_PATH.into(),
        handoff: page.scribe_handoff.unwrap_or(false),
        body: (!private).then_some(page.body),
    })
}

/// What happened to a crew-written scribe handoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandoffOutcome {
    /// The Markdown with the `handoff: (scribe) …` entry appended.
    Written(String),
    /// Dropped for the given reason; the pass's other entries still land.
    Dropped(&'static str),
}

/// `#L<n>` and `#L<n>-<m>` anchors cited in `text`, or `None` for a
/// malformed one (no digits, overflow or a reversed range).
fn cited_anchors(text: &str) -> Option<Vec<usize>> {
    let mut lines = Vec::new();
    for (at, _) in text.match_indices("#L") {
        let rest = &text[at + 2..];
        let digits = |s: &str| -> Option<(usize, usize)> {
            let len = s.chars().take_while(char::is_ascii_digit).count();
            (len > 0).then(|| s[..len].parse().ok().map(|n| (n, len)))?
        };
        let (start, len) = digits(rest)?;
        lines.push(start);
        if let Some(range) = rest[len..].strip_prefix('-')
            && let Some((end, _)) = digits(range)
        {
            if end < start {
                return None;
            }
            lines.push(end);
        }
    }
    Some(lines)
}

/// Append a crew-written scribe handoff to `merged` (the source after this
/// pass's entries were merged into `base`). It lands only when the pass
/// added a decision, direction or action, the text is one line of at most
/// [`HANDOFF_WORD_LIMIT`] words, and it cites at least one `#L<n>` anchor
/// of the merged source with every cited anchor naming a log entry. It is
/// written as kind `handoff` with a leading `(scribe)` marker, so every
/// existing log reader accepts it and the session brief shows the marker.
pub fn append_handoff(base: &str, merged: &str, text: &str, at: &str) -> Result<HandoffOutcome> {
    let text = scrub(text);
    let text = text.trim();
    let text = text.strip_prefix("(scribe)").unwrap_or(text).trim();
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Ok(HandoffOutcome::Dropped("invalid"));
    }
    if text.split_whitespace().count() > HANDOFF_WORD_LIMIT {
        return Ok(HandoffOutcome::Dropped("too_long"));
    }
    let before = super::source_body(base).lines().count();
    let body = super::source_body(merged);
    let log = crate::domain::session::entries(&body)?;
    if !log.iter().any(|entry| {
        entry.line > before && ["decision", "direction", "action"].contains(&entry.kind.as_str())
    }) {
        return Ok(HandoffOutcome::Dropped("no_plan_change"));
    }
    let Some(cited) = cited_anchors(text) else {
        return Ok(HandoffOutcome::Dropped("unresolved_anchor"));
    };
    if cited.is_empty() {
        return Ok(HandoffOutcome::Dropped("no_anchor"));
    }
    if !cited
        .iter()
        .all(|line| log.iter().any(|entry| entry.line == *line))
    {
        return Ok(HandoffOutcome::Dropped("unresolved_anchor"));
    }
    let entry = format_conversation_entry(at, "handoff", &format!("(scribe) {text}"))?;
    let mut result = merged.to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&entry);
    result.push('\n');
    Ok(HandoffOutcome::Written(result))
}

/// Apply the request's scribe handoff, if any, to `merged`; returns the
/// Markdown to write and the handoff outcome token for the response.
pub fn with_handoff(
    request: &Request,
    session: &Session,
    base: &str,
    merged: String,
    at: &str,
) -> Result<(String, Option<String>)> {
    let Some(handoff) = &request.handoff else {
        return Ok((merged, None));
    };
    // A private session never reaches the crew, so it has no handoff.
    if session.sensitivity == "private" {
        return Ok((merged, Some("private".into())));
    }
    Ok(match append_handoff(base, &merged, handoff, at)? {
        HandoffOutcome::Written(text) => (text, Some("written".into())),
        HandoffOutcome::Dropped(reason) => (merged, Some(reason.into())),
    })
}

/// Where the session's conversation source stands, for an incremental
/// preview: where the next pass starts and which anchor its first new
/// entry will take.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cursor {
    /// Source id.
    pub id: String,
    /// Library-relative path of `source.md`.
    pub path: String,
    /// Last transcript line an earlier pass considered; `None` before the
    /// incremental scheme. The next pass uses `from_line` one past it.
    pub extracted_through: Option<usize>,
    /// One-based body line (`#L<n>`) the next appended entry will take.
    pub next_anchor: usize,
}

/// The cursor of the conversation source recording `session_id`, if any.
/// Several sources with one session id refuse, as capture does.
pub fn cursor(sources: &[crate::ports::LibraryText], session_id: &str) -> Result<Option<Cursor>> {
    let mut found = None;
    for file in sources {
        let Some(markdown) = file.markdown.as_deref() else {
            continue;
        };
        let fields = crate::domain::metrics::metadata(markdown)?;
        if fields.get("session_id").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        if found.is_some() {
            return Err(Error::Library {
                code: "session_source_conflict",
                message: "multiple sources have this session id; reconcile them before capture"
                    .into(),
            });
        }
        let (extracted_through, _) = extracted_state(markdown)?;
        found = Some(Cursor {
            id: super::parse_source_meta(markdown).id,
            path: file.path.clone(),
            extracted_through,
            next_anchor: super::source_body(markdown).lines().count() + 1,
        });
    }
    Ok(found)
}

/// The user's own scribe rules page among `pages`, if it exists.
pub fn find_rules(pages: &[crate::ports::LibraryText]) -> Result<Option<Rules>> {
    pages
        .iter()
        .find(|file| file.path == RULES_PAGE_PATH)
        .map(|file| {
            file.markdown
                .as_deref()
                .ok_or_else(|| Error::Library {
                    code: "library_document_unreadable",
                    message: format!("cannot read {}", file.path),
                })
                .and_then(rules)
        })
        .transpose()
}
