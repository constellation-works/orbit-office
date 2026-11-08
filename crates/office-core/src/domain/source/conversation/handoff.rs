//! Scribe rules, handoffs and incremental cursors.

use serde::Serialize;
use serde_json::Value;

use super::super::format_conversation_entry;
use super::provenance::extracted_state;
use super::scrub::scrub;
use super::types::{Request, Session};
use crate::{Error, Result};

/// Library path of the scribe rules page (`dom-scribe-rules`).
pub const RULES_PAGE_PATH: &str = "knowledge/domain/dom-scribe-rules.md";

/// The scribe rules page as the crew and the adapter see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rules {
    /// Always `dom-scribe-rules`.
    pub id: String,
    /// Library-relative path.
    pub path: String,
    /// `scribe.handoff`: retired with scribe v2, whose SCR is the handoff
    /// (see the scribe removal in 4_decisions.md), so always false. A page that
    /// still sets it stays valid; the migration proposes removing it.
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
        handoff: false,
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

/// Append a validated structured scribe handoff after a plan-changing pass.
/// Legacy prose and invalid structures are dropped with an explicit reason.
/// Source anchors must resolve, and the stored line retains `(scribe)`.
pub fn append_handoff(base: &str, merged: &str, text: &str, at: &str) -> Result<HandoffOutcome> {
    let text = scrub(text);
    let text = text.trim();
    let text = text.strip_prefix("(scribe)").unwrap_or(text).trim();
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Ok(HandoffOutcome::Dropped("invalid"));
    }
    let before = super::super::source_body(base).lines().count();
    let body = super::super::source_body(merged);
    let log = crate::domain::session::entries(&body)?;
    if !log.iter().any(|entry| {
        entry.line > before && ["decision", "direction", "action"].contains(&entry.kind.as_str())
    }) {
        return Ok(HandoffOutcome::Dropped("no_plan_change"));
    }
    let handoff = match crate::domain::session::handoff::decode(text) {
        Ok(Some(handoff)) => handoff,
        Ok(None) => return Ok(HandoffOutcome::Dropped("structured_required")),
        Err(error) => return Ok(HandoffOutcome::Dropped(error.code())),
    };
    let rendered = handoff.render();
    let Some(cited) = cited_anchors(&rendered) else {
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
    let entry =
        format_conversation_entry(at, "handoff", &format!("(scribe) {}", handoff.encode()?))?;
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
pub fn cursor(
    sources: &[crate::domain::source::LibraryText],
    session_id: &str,
) -> Result<Option<Cursor>> {
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
            id: super::super::parse_source_meta(markdown).id,
            path: file.path.clone(),
            extracted_through,
            next_anchor: super::super::source_body(markdown).lines().count() + 1,
        });
    }
    Ok(found)
}

/// The user's own scribe rules page among `pages`, if it exists.
pub fn find_rules(pages: &[crate::domain::source::LibraryText]) -> Result<Option<Rules>> {
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
