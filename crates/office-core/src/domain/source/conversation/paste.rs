//! Paste wrappers, side-session briefs and record titles.

use serde_json::Value;

use super::scrub::scrub;

/// The paste wrapper a provider or harness puts around pasted text.
const PASTE_TAG: &str = "pasted_content";

/// Whether a trimmed line opens a paste wrapper (`<pasted_content` followed
/// by `>`, whitespace or an attribute), case-insensitively.
pub(crate) fn opens_paste(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower
        .strip_prefix('<')
        .and_then(|rest| rest.strip_prefix(PASTE_TAG))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['>', ' ', '\t', '/']))
}

/// Whether a line closes a paste wrapper.
pub(crate) fn closes_paste(line: &str) -> bool {
    line.to_ascii_lowercase()
        .contains(&format!("</{PASTE_TAG}>"))
}

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
pub(crate) fn record_title(record: &Value, kind: &str) -> Option<String> {
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
