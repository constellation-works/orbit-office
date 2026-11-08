use std::path::Path;

use super::decision::offset_time;
use super::types::{Context, DecidedBy, Options};
use super::{Decision, Scope};
use crate::Result;
use crate::domain::knowledge::DocumentRecord;

/// The scope a migrated note goes to: a `project:<name>` or
/// `scope:project:<name>` tag names a project, otherwise `global`.
#[must_use]
pub fn note_scope(tags: &[String]) -> Scope {
    tags.iter()
        .find_map(|tag| {
            let tag = tag.strip_prefix("scope:").unwrap_or(tag);
            tag.starts_with("project:")
                .then(|| Scope::parse(tag).ok())
                .flatten()
        })
        .unwrap_or(Scope::Global)
}

/// A retired `note` source, as the migration reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteSource {
    /// Its `src-…` id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its body, verbatim.
    pub body: String,
    /// The ledger scope it moves to ([`note_scope`]).
    pub scope: Scope,
    /// `occurred_at`, else `captured_at`, else `fallback`.
    pub decided_at: String,
    /// Whether the note is private (left in place by the migration).
    pub private: bool,
}

/// Read a `source.md` as a retired note; `None` for any other kind.
#[must_use]
pub fn note_source(path: &str, markdown: &str, fallback: &str) -> Option<NoteSource> {
    use crate::domain::knowledge::frontmatter;
    let meta = crate::domain::source::parse_source_meta(markdown);
    if meta.kind != "note" || meta.id.is_empty() {
        return None;
    }
    let fields = frontmatter::split(markdown)
        .and_then(|(header, _)| frontmatter::parse(header))
        .unwrap_or_default();
    let decided_at = ["occurred_at", "captured_at"]
        .iter()
        .find_map(|key| {
            fields
                .iter()
                .find(|(name, _)| name == key)
                .and_then(|(_, value)| value.as_str())
                .filter(|value| offset_time(value))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| fallback.to_owned());
    let record = DocumentRecord::from_markdown(Path::new(path), markdown);
    Some(NoteSource {
        id: meta.id,
        title: meta.title,
        body: crate::domain::source::source_body(markdown),
        scope: note_scope(&record.tags),
        decided_at,
        private: meta.sensitivity == "private",
    })
}

/// The ledger entry a retired `note` source becomes (specs/decisions.md,
/// "Retiring `sources/note`"): the title restated as the question, options
/// unrecoverable, the body verbatim as the quote, its first line as the
/// chosen span, `decided_by: human` and `source` set to the note's id.
pub fn note_entry(
    scope: &Scope,
    id: &str,
    source_id: &str,
    title: &str,
    body: &str,
    decided_at: &str,
) -> Result<Decision> {
    let quote = body.trim_end_matches('\n').to_string();
    let chosen = quote
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string();
    let lines = quote.lines().count().max(1);
    let question = if title.trim().is_empty() {
        chosen.clone()
    } else {
        title.trim().replace(['\n', '\r'], " ")
    };
    let decision = Decision {
        id: id.into(),
        question: Some(question),
        question_ref: None,
        question_by: Some("scribe".into()),
        options: Some(Options::Unrecoverable),
        chosen: Some(chosen),
        quote: Some(quote),
        reference: if lines == 1 {
            format!("{source_id}#L1")
        } else {
            format!("{source_id}#L1-{lines}")
        },
        because: None,
        scope: scope.token(),
        context: Context::default(),
        decided_by: DecidedBy::Human,
        decided_at: decided_at.into(),
        reverses: None,
        orbit: None,
        same_as: None,
        confirms: None,
        source: source_id.into(),
        private: false,
    };
    decision.validate()?;
    Ok(decision)
}
