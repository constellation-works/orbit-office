//! Shared parsing and citation helpers for the session brief.
use serde_json::Value;

use crate::domain::metrics::timestamp;
use crate::domain::source::LibraryText;
use crate::{Error, Result};

/// Invalid or incomplete evidence cannot look like an empty workspace.
#[must_use]
pub fn invalid(field: &str) -> Error {
    Error::Library {
        code: "session_brief_data_invalid",
        message: format!("session brief requires valid recorded {field}"),
    }
}

/// Exact Markdown from a port read, refusing an unreadable file by path.
pub fn markdown(file: &LibraryText) -> Result<&str> {
    file.markdown.as_deref().ok_or_else(|| Error::Library {
        code: "library_document_unreadable",
        message: format!("cannot read {}", file.path),
    })
}

pub(super) fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(field))
}

// Compare instants, not differently offset timestamp strings. Preserve arbitrary
// fractional precision as well as the original spelling returned to the caller.
pub(super) fn time_key(at: &str) -> Result<(i64, String)> {
    let seconds = timestamp(at).ok_or_else(|| invalid("timestamp"))?;
    let fraction = at
        .get(19..)
        .and_then(|s| s.strip_prefix('.'))
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .unwrap_or_default();
    Ok((seconds, fraction.trim_end_matches('0').into()))
}

/// An optional single-line frontmatter field: absent or null is `None`, a
/// nonempty string is kept, anything else refuses the read.
pub(super) fn optional_text(fields: &Value, field: &str) -> Result<Option<String>> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(invalid(field)),
    }
}

/// Normalize a project value to the repository name: a trailing
/// `.claude/worktrees/<name>` names its repository, otherwise the last path
/// component; a bare name is unchanged.
pub(super) fn project_key(project: Option<&str>) -> Option<String> {
    let project = project?;
    let parts: Vec<&str> = project
        .trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() >= 4
        && parts[parts.len() - 2] == "worktrees"
        && parts[parts.len() - 3] == ".claude"
    {
        return Some(parts[parts.len() - 4].to_string());
    }
    parts.last().map(|part| (*part).to_string())
}

/// The first Orbit assignment id a handoff cites (`ORB-…`, `DANI-…` or
/// `ORBA-…`), when it cites one. The id is used directly as the thread key;
/// the brief does not verify the task's type.
pub(super) fn cited_assignment(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !text.is_char_boundary(at) {
            at += 1;
            continue;
        }
        for prefix in ["ORBA-", "DANI-", "ORB-"] {
            if text[at..].starts_with(prefix)
                && (at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'-'))
            {
                let start = at + prefix.len();
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-')
                {
                    end += 1;
                }
                let id = text[at..end].trim_end_matches('-');
                if id.len() > prefix.len() && id.bytes().any(|b| b.is_ascii_digit()) {
                    return Some(id.to_string());
                }
            }
        }
        at += 1;
    }
    None
}

/// Whether `text` cites `anchor` as a whole token: the next character is not
/// a digit and, for a local `#L<n>`, the previous one does not continue a
/// source id.
pub(super) fn cites(text: &str, anchor: &str) -> bool {
    let mut from = 0;
    while let Some(found) = text[from..].find(anchor) {
        let at = from + found;
        let end = at + anchor.len();
        let next_ok = !text[end..].starts_with(|c: char| c.is_ascii_digit());
        let previous_ok = !anchor.starts_with('#')
            || !text[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if next_ok && previous_ok {
            return true;
        }
        from = end;
    }
    false
}
