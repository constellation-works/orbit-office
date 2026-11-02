//! Model-free marker detection (specs/scribe.md, "Markers", "Trust rule").
//!
//! A marker starts a line of a parsed message. Fenced blocks and
//! blockquotes never reach the parse (the adapter drops them); lines inside
//! a paste region the adapter saw are skipped here. Markers in the user's
//! typed turns are the trusted list; markers in assistant text are kept
//! apart, recorded `by: agent` and never authoritative.

use serde::Serialize;

use super::super::{Message, topic_of};
use crate::domain::scribe_settings::{ScribeTag, TagScope};
use crate::{Error, Result};

/// A user turn longer than this many lines is paste-sized: its markers
/// never take apply mode.
pub const TURN_MAX_LINES: usize = 40;
/// A user turn longer than this many characters is paste-sized.
pub const TURN_MAX_CHARS: usize = 4000;

/// Who wrote a marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Author {
    /// The user's own typed turn.
    User,
    /// Assistant text: recorded `by: agent`, never authoritative.
    Agent,
}

impl Author {
    /// `user` or `agent`, as written on a Tags entry.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

/// One marker found in a parsed message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Marker {
    /// The message id.
    pub msg: String,
    /// 1-based line of the message text the marker starts.
    pub line: usize,
    /// The marker as written, lowercased (`#decision`, `#scribe`, …).
    pub marker: String,
    /// The tag it records; `None` for a `#scribe <text>` instruction.
    pub tag: Option<String>,
    /// `global` or `project:<name>`: the marked scope, else the default.
    pub scope: String,
    /// Whether the marker named its scope (`@global`, `@<project>`).
    pub scope_marked: bool,
    /// The text after the marker, tag and scope, before any `because:`.
    pub text: String,
    /// The stated rationale after `because:` on the marker line.
    pub because: Option<String>,
    /// The marker line verbatim.
    pub quote: String,
    /// Who wrote it.
    pub by: Author,
    /// The message time.
    pub at: String,
    /// Whether the tag is defined in the Library settings.
    pub in_force: bool,
    /// Whether the turn is paste-sized (over 40 lines or 4,000 characters).
    pub large_turn: bool,
}

impl Marker {
    /// The reference view returned for a private session: no text.
    #[must_use]
    pub fn reference(&self) -> serde_json::Value {
        serde_json::json!({
            "msg": self.msg, "line": self.line, "marker": self.marker, "tag": self.tag,
            "scope": self.scope, "scope_marked": self.scope_marked, "by": self.by,
            "at": self.at, "in_force": self.in_force, "large_turn": self.large_turn,
            "content_withheld": true,
        })
    }
}

/// The session's scope token: `global` or `project:<name>`. `requested`
/// is `global`, `project` or `project:<name>`; absent, the session's
/// project, else global.
pub fn session_scope(project: Option<&str>, requested: Option<&str>) -> Result<String> {
    let project_scope = || {
        project
            .and_then(topic_of)
            .map(|slug| format!("project:{slug}"))
    };
    match requested {
        None => Ok(project_scope().unwrap_or_else(|| "global".to_owned())),
        Some("global") => Ok("global".to_owned()),
        Some("project") => project_scope()
            .ok_or_else(|| Error::InvalidArgument("scope project needs a session project".into())),
        Some(other) => match other.strip_prefix("project:").and_then(topic_of) {
            Some(slug) if other == format!("project:{slug}") => Ok(other.to_owned()),
            _ => Err(Error::InvalidArgument(
                "scope is global, project or project:<name> with a lowercase kebab-case name"
                    .into(),
            )),
        },
    }
}

/// The tag a marker word names, after `#`; aliases resolve to the default.
fn marker_tag(word: &str) -> Option<&'static str> {
    match word {
        "preferences" | "preference" => Some("preferences"),
        "decisions" | "decision" => Some("decisions"),
        _ => None,
    }
}

/// The first whitespace-separated token and the rest, trimmed.
fn split_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim_start()),
        None => (text, ""),
    }
}

fn default_scope(tag: Option<&str>, tags: &[ScribeTag], session: &str) -> String {
    match tag {
        Some("preferences") => "global".to_owned(),
        Some(name) => match tags.iter().find(|t| t.name == name).map(|t| t.scope) {
            Some(TagScope::Global) => "global".to_owned(),
            Some(TagScope::Project | TagScope::Session) | None => session.to_owned(),
        },
        None => session.to_owned(),
    }
}

/// One line read as a marker.
struct LineMarker {
    marker: String,
    tag: Option<String>,
    scope: String,
    scope_marked: bool,
    text: String,
    because: Option<String>,
}

/// Parse one trimmed line as a marker; `None` when it is not one.
fn parse_line(line: &str, tags: &[ScribeTag], session: &str) -> Option<LineMarker> {
    let rest = line.strip_prefix('#')?;
    let (word, after) = split_token(rest);
    let word = word.to_ascii_lowercase();
    let (tag, after): (Option<String>, &str) = if word == "scribe" {
        let (next, tail) = split_token(after);
        let named = marker_tag(next)
            .map(str::to_owned)
            .or_else(|| tags.iter().find(|t| t.name == next).map(|t| t.name.clone()));
        match named {
            Some(name) if !next.is_empty() => (Some(name), tail),
            _ => (None, after),
        }
    } else {
        (Some(marker_tag(&word)?.to_owned()), after)
    };
    let mut scope = default_scope(tag.as_deref(), tags, session);
    let mut scope_marked = false;
    let mut after = after;
    let (token, tail) = split_token(after);
    if let Some(name) = token.strip_prefix('@') {
        if name == "global" {
            scope = "global".to_owned();
            scope_marked = true;
            after = tail;
        } else if let Some(slug) = topic_of(name) {
            scope = format!("project:{slug}");
            scope_marked = true;
            after = tail;
        }
    }
    let lower = after.to_ascii_lowercase();
    let (text, because) = match lower.find("because:") {
        Some(at) => {
            let reason = after[at + "because:".len()..].trim();
            (
                after[..at].trim().to_owned(),
                (!reason.is_empty()).then(|| reason.to_owned()),
            )
        }
        None => (after.trim().to_owned(), None),
    };
    Some(LineMarker {
        marker: format!("#{word}"),
        tag,
        scope,
        scope_marked,
        text,
        because,
    })
}

/// Every marker in `messages[from..=through]`, split into the user's
/// (trusted) and the assistant's (`by: agent`). `session` is the session's
/// scope token; `tags` are the definitions in force.
#[must_use]
pub fn detect(
    messages: &[Message],
    from: usize,
    through: Option<usize>,
    tags: &[ScribeTag],
    session: &str,
) -> (Vec<Marker>, Vec<Marker>) {
    let mut user = Vec::new();
    let mut agent = Vec::new();
    let Some(through) = through else {
        return (user, agent);
    };
    for message in messages.iter().take(through + 1).skip(from) {
        let by = if message.role == "user" {
            Author::User
        } else {
            Author::Agent
        };
        let large_turn = message.text.lines().count() > TURN_MAX_LINES
            || message.text.chars().count() > TURN_MAX_CHARS;
        let mut fenced = false;
        for (index, raw) in message.text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            // The adapter already drops fences and blockquotes; a message
            // file written another way gets the same rule.
            if line.starts_with("```") || line.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if fenced || line.starts_with('>') || message.pasted.contains(&number) {
                continue;
            }
            let Some(LineMarker {
                marker,
                tag,
                scope,
                scope_marked,
                text,
                because,
            }) = parse_line(line, tags, session)
            else {
                continue;
            };
            let in_force = tag
                .as_deref()
                .is_none_or(|name| tags.iter().any(|t| t.name == name));
            let found = Marker {
                msg: message.id.clone(),
                line: number,
                marker,
                tag,
                scope,
                scope_marked,
                text,
                because,
                quote: line.to_owned(),
                by,
                at: message.at.clone(),
                in_force,
                large_turn,
            };
            match by {
                Author::User => user.push(found),
                Author::Agent => agent.push(found),
            }
        }
    }
    (user, agent)
}
