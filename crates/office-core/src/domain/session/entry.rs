//! Decision-log entry parsing.
use crate::Result;
use crate::domain::source::is_conversation_kind;

use super::handoff;
use super::helpers::{invalid, time_key};

/// A quoted, anchorable decision-log entry. Kinds use the capture vocabulary,
/// including `fact`; the vocabulary is defined once in `source`.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Body-relative, one-based line number, matching `source-show` anchors.
    pub line: usize,
    /// Original RFC 3339 timestamp, including its offset.
    pub time: String,
    /// Capture's validated entry-kind token.
    pub kind: String,
    /// Exact text after the kind delimiter, without trimming or interpolation.
    pub text: String,
    /// Whether one or more corrections changed this entry.
    pub corrected: bool,
    /// Explicit user attribution, when present (a claim, not authority).
    pub by: Option<String>,
}

/// Parse the append-only log. Ordinary prose and blank lines still count for
/// anchors. A malformed timestamped entry refuses the complete read.
pub fn entries(body: &str) -> Result<Vec<Entry>> {
    let mut result = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let (time, entry) = rest.split_once("] ").ok_or_else(|| invalid("log entry"))?;
        let (kind, text, by) = if let Some((kind, text)) = entry.split_once(" (by: user): ")
            && is_conversation_kind(kind)
        {
            let text: String =
                serde_json::from_str(text).map_err(|_| invalid("user entry text"))?;
            crate::domain::source::conversation::corrections::validate_user_text(
                kind, &text, "user",
            )?;
            (kind, text, Some("user".to_string()))
        } else {
            let (kind, text) = entry.split_once(": ").ok_or_else(|| invalid("log entry"))?;
            (kind, text.to_string(), None)
        };
        time_key(time)?;
        if !is_conversation_kind(kind) || text.trim().is_empty() {
            return Err(invalid("log entry kind or text"));
        }
        if kind == "handoff" {
            handoff::decode(&text)?;
        }
        result.push(Entry {
            line: index + 1,
            time: time.into(),
            kind: kind.into(),
            text,
            corrected: false,
            by,
        });
    }
    Ok(result)
}
