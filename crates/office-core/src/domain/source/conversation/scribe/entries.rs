//! Append inputs, shape checking and stored entry references.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Front, invalid};
use crate::Result;

/// The note form of a `why`: `true` (the reason is quoted from the entry's
/// own note text) or the reason text itself.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum WhyNote {
    /// `note: true`: the quote comes from the entry's own note text.
    Flag(bool),
    /// `note: "<text>"`: the reason text, quoted exactly.
    Text(String),
}

/// Why a decision was made: a cited message or a note, with its verbatim
/// reason quote.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Why {
    /// Message giving the reason.
    #[serde(default)]
    pub msg: Option<String>,
    /// Note giving the reason.
    #[serde(default)]
    pub note: Option<WhyNote>,
    /// Exact substring of the cited message or note.
    #[serde(default)]
    pub quote: Option<String>,
}

/// One scribe entry: a cited message (`msg` and `quote`) or a note (`text`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Cited message id.
    #[serde(default)]
    pub msg: Option<String>,
    /// Exact substring of the cited message.
    #[serde(default)]
    pub quote: Option<String>,
    /// Note text, written verbatim in place of a quote.
    #[serde(default)]
    pub text: Option<String>,
    /// One allowed tag.
    pub tag: String,
    /// Allowed topics.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Task, PR or commit references.
    #[serde(default)]
    pub refs: Vec<String>,
    /// Due time or date.
    #[serde(default)]
    pub due: Option<String>,
    /// `<SCR id>#L<n>` of the pending line a `resolved` entry closes.
    #[serde(default)]
    pub resolves: Option<String>,
    /// `user` or a crew name.
    #[serde(default)]
    pub by: Option<String>,
    /// Why a `decision`, `agent-decision` or `preference` was made.
    #[serde(default)]
    pub why: Option<Why>,
    /// Matter slug a `preference` settles.
    #[serde(default)]
    pub matter: Option<String>,
    /// When a `preference` comes up for review; 90 days after the entry
    /// by default.
    #[serde(default)]
    pub review_by: Option<String>,
    /// `<SCR id>#L<n>` of the preference line this preference replaces.
    #[serde(default)]
    pub supersedes: Option<String>,
}

/// Parse and shape-check the `entries` input.
pub fn parse_entries(value: Option<&Value>) -> Result<Vec<Entry>> {
    let items = match value {
        None => return Ok(vec![]),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(invalid("entries must be an array")),
    };
    items
        .iter()
        .map(|item| {
            let entry: Entry = serde_json::from_value(item.clone()).map_err(|_| {
                invalid(
                    "each entry is {msg, quote} or {text}, with tag and optional topics, refs, due, resolves, by, why, matter, review_by and supersedes; no other keys",
                )
            })?;
            match (&entry.msg, &entry.quote, &entry.text) {
                (Some(_), Some(_), None) | (None, None, Some(_)) => {}
                _ => {
                    return Err(invalid(
                        "an entry cites a message with msg and quote, or is a note with text alone",
                    ));
                }
            }
            if let Some(why) = &entry.why {
                let shape_ok = match (&why.msg, &why.note, &why.quote) {
                    (Some(_), None, Some(quote)) => !quote.is_empty(),
                    (None, Some(WhyNote::Flag(true)), Some(quote)) => !quote.is_empty(),
                    (None, Some(WhyNote::Text(text)), quote) => {
                        !text.is_empty() && quote.as_ref().is_none_or(|q| !q.is_empty())
                    }
                    _ => false,
                };
                if !shape_ok {
                    return Err(invalid(
                        "why is {msg, quote} or {note, quote} with a verbatim reason quote",
                    ));
                }
            }
            Ok(entry)
        })
        .collect()
}

/// An entry append did not write, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// Position in the input.
    pub index: usize,
    /// Cited message, or `note`.
    pub msg: String,
    /// Stable reason code.
    pub reason: &'static str,
    /// Human-readable detail, without message text.
    pub message: String,
}

/// What [`super::append()`] decided.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Final id.
    pub id: String,
    /// Final path.
    pub path: String,
    /// The existing file, when updating.
    pub previous: Option<(String, String)>,
    /// New Markdown, or `None` when nothing changes.
    pub markdown: Option<String>,
    /// Lines added, with their 1-based file line numbers.
    pub added: Vec<(usize, String)>,
    /// Entries already present, skipped.
    pub duplicates: usize,
    /// Entries refused.
    pub refused: Vec<Refusal>,
    /// The id before a rename.
    pub renamed_from: Option<String>,
    /// Cursor before this append.
    pub previous_through: Option<String>,
    /// Front matter after this append.
    pub front: Front,
}

/// Inputs of one append besides the entries.
pub struct Request<'a> {
    /// Provider session id.
    pub session_id: &'a str,
    /// Last message given to the agent; `None` for a note/title-only
    /// append, which leaves `scribed_through` unchanged.
    pub through: Option<&'a str>,
    /// New title, renaming the file.
    pub title: Option<&'a str>,
    /// Who sets `title`: `user` (default), `custom` or `scribe`.
    pub title_by: Option<&'a str>,
    /// Counterpart kind.
    pub counterpart: Option<&'a str>,
    /// Counterpart name.
    pub counterpart_name: Option<&'a str>,
    /// Current time, RFC 3339.
    pub now: &'a str,
    /// Host time zone (IANA), UTC when `None`.
    pub zone: Option<&'a str>,
}

/// The tag of the entry on line `n` (1-based) of `markdown`, without `#`.
pub(super) fn tag_at(markdown: &str, n: usize) -> Option<String> {
    let line = n
        .checked_sub(1)
        .and_then(|index| markdown.lines().nth(index))?;
    let mut parts = line.split(' ');
    (parts.next()? == "-")
        .then(|| parts.nth(2))
        .flatten()
        .and_then(|tag| tag.strip_prefix('#'))
        .map(str::to_owned)
}

/// The `matter:` value on line `n` (1-based) of `markdown`.
pub(super) fn matter_at(markdown: &str, n: usize) -> Option<String> {
    let line = n
        .checked_sub(1)
        .and_then(|index| markdown.lines().nth(index))?;
    line.split(' ')
        .zip(line.split(' ').skip(1))
        .find(|(key, _)| *key == "matter:")
        .map(|(_, value)| value.to_owned())
}
