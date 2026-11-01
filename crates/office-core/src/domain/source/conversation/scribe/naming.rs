//! SCR name selection, title precedence and session start.

use chrono::{DateTime, FixedOffset};

use super::{Message, Parsed, ScrFile, local, refusal, scr_id, scr_path, slug, with_suffix};
use crate::Result;

fn taken(files: &[ScrFile], id: &str, session_id: &str) -> bool {
    files.iter().any(|file| {
        file.front.session_id != session_id
            && (file.front.id == id
                || file.front.aliases.iter().any(|alias| alias == id)
                || file.path.ends_with(&format!("/{id}.md")))
    })
}

/// Pick a free id: the plain id, else with the session suffix.
pub fn free_id(files: &[ScrFile], id: &str, session_id: &str) -> Result<String> {
    if !taken(files, id, session_id) {
        return Ok(id.to_owned());
    }
    let suffixed = with_suffix(id, session_id);
    if taken(files, &suffixed, session_id) {
        return Err(refusal(
            "scribe_name_conflict",
            "the SCR name and its session-suffixed form are both taken",
        ));
    }
    Ok(suffixed)
}

/// The slug source of an untitled session: the first user message with
/// remaining text. A message emptied by harness stripping is skipped; a
/// bare slash command counts when nothing else exists. A side-session
/// brief is an assistant message, so it never names the file here.
#[must_use]
pub fn first_words(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user" && !m.text.trim().is_empty())
        .or_else(|| messages.iter().find(|m| !m.text.trim().is_empty()))
        .and_then(|m| m.text.lines().find(|line| !line.trim().is_empty()))
        .map(slug)
        .unwrap_or_default()
}

/// The side-session brief among `messages`, if the session has one.
fn brief_message(messages: &[Message]) -> Option<&Message> {
    messages
        .iter()
        .find(|m| super::super::is_side_session_brief(&m.text))
}

/// The desktop app's placeholder title for a session nobody has named:
/// `New session`, case-insensitive, with or without one trailing number.
/// It names nothing, so it counts as no title.
fn placeholder_title(title: &str) -> bool {
    let mut words = title.split_whitespace();
    if !words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("new"))
        || !words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("session"))
    {
        return false;
    }
    let rest: Vec<&str> = words.collect();
    rest.len() <= 1
        && rest
            .iter()
            .all(|word| word.bytes().all(|b| b.is_ascii_digit()))
}

/// A title that can name the file: non-blank and not a placeholder.
pub(super) fn named_title(title: Option<&str>) -> Option<&str> {
    title.filter(|t| !t.trim().is_empty() && !placeholder_title(t))
}

/// Humanized filename slug of an SCR id (`SCR-30-1135-pilot-admission-hold`
/// becomes `Pilot admission hold`), for display where the title is still null.
#[must_use]
pub fn humanized_title(id: &str) -> Option<String> {
    let slug = id
        .strip_prefix("SCR-")
        .and_then(|rest| rest.split_once('-'))
        .and_then(|(_, rest)| rest.split_once('-'))
        .map(|(_, slug)| slug)?;
    let words = slug.replace('-', " ");
    let words = words.trim();
    if words.is_empty() {
        return None;
    }
    let mut chars = words.chars();
    let first = chars.next()?.to_uppercase().collect::<String>();
    Some(format!("{first}{}", chars.collect::<String>()))
}

/// Title a caller displays for a session: its real title, else the
/// humanized filename slug. Never `Untitled session`.
#[must_use]
pub fn display_title(title: Option<&str>, id: &str) -> Option<String> {
    match title {
        Some(title) if !title.trim().is_empty() => Some(title.to_owned()),
        _ => humanized_title(id),
    }
}

/// Transcript-derived title: a real custom title, else a side session's
/// real title/summary record. Both are `custom`, never placeholder.
pub(super) fn derived_title(
    parsed: &Parsed,
    messages: &[Message],
) -> Option<(String, &'static str)> {
    if let Some(title) = named_title(parsed.custom_title.as_deref()) {
        return Some((title.to_owned(), "custom"));
    }
    if brief_message(messages).is_some()
        && let Some(title) = named_title(parsed.session_title.as_deref())
    {
        return Some((title.to_owned(), "custom"));
    }
    None
}

/// Slug source for a new file: an explicit title wins, then a set
/// custom title (the latest `customTitle`, unless it is a placeholder)
/// for every session; a side session then takes the transcript's
/// title/summary record (again, unless a placeholder), else its brief's
/// task-title line, else the brief's first prose sentence after the
/// preamble (never the preamble itself); every other session takes
/// Daniel's first message.
fn planned_slug(parsed: &Parsed, messages: &[Message], title: Option<&str>) -> String {
    if let Some(title) = title {
        return slug(title);
    }
    if let Some(title) = named_title(parsed.custom_title.as_deref()) {
        let value = slug(title);
        if !value.is_empty() {
            return value;
        }
    }
    if let Some(brief) = brief_message(messages) {
        if let Some(title) = named_title(parsed.session_title.as_deref()) {
            let value = slug(title);
            if !value.is_empty() {
                return value;
            }
        }
        if let Some(task) = super::super::brief_task_title(&brief.text) {
            let value = slug(&task);
            if !value.is_empty() {
                return value;
            }
        }
        if let Some(sentence) = super::super::brief_first_sentence(&brief.text) {
            let value = slug(&sentence);
            if !value.is_empty() {
                return value;
            }
        }
    }
    first_words(messages)
}

/// Session start as written in front matter and used for the name.
pub fn session_start(
    parsed: &Parsed,
    messages: &[Message],
    zone: Option<&str>,
) -> Result<DateTime<FixedOffset>> {
    let at = parsed
        .started_at
        .as_deref()
        .or_else(|| messages.first().map(|m| m.at.as_str()))
        .ok_or_else(|| refusal("scribe_no_messages", "the session has no messages yet"))?;
    local(at, zone)
}

/// The name and path a new SCR file for this session takes.
pub fn planned(
    files: &[ScrFile],
    parsed: &Parsed,
    messages: &[Message],
    title: Option<&str>,
    zone: Option<&str>,
) -> Result<(String, String)> {
    let start = session_start(parsed, messages, zone)?;
    let slug = planned_slug(parsed, messages, title);
    let id = free_id(files, &scr_id(start, &slug), &parsed.session_id)?;
    let path = scr_path(start, &id);
    Ok((id, path))
}
