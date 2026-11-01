//! `office.scribe-query`: model-free filters over the session SCR lines
//! (specs/scribe.md, "Answering").
//!
//! Read-only. Reads every SCR file through the Library port, parses its
//! entry lines, applies the canned mode and the general filters, and
//! resolves the links between lines: a `resolved` line closes the
//! `pending` line it names, a preference is current unless a later
//! preference on the same matter supersedes it (and is `stale` past its
//! `review_by`), and a `#why` note fills the `#no-why` decision it names.
//! It never touches the Models port.

use std::collections::BTreeMap;

use office_core::domain::source::conversation::scribe::{self, ScrFile};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "scribe-query",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "mode",
        "tag",
        "matter",
        "topic",
        "by",
        "since",
        "until",
        "refs",
        "session_id",
        "limit",
    ],
    run,
);

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const MODES: &[&str] = &["pending", "pref", "landed", "why"];

/// One parsed SCR entry line, with its file's front matter.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) scr_id: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) text: String,
    pub(crate) msg: String,
    pub(crate) time: String,
    pub(crate) tag: String,
    pub(crate) topics: Vec<String>,
    pub(crate) no_why: bool,
    pub(crate) quote: String,
    pub(crate) why_source: Option<String>,
    pub(crate) why_quote: Option<String>,
    pub(crate) refs: Vec<String>,
    pub(crate) due: Option<String>,
    pub(crate) matter: Option<String>,
    pub(crate) review_by: Option<String>,
    pub(crate) supersedes: Option<String>,
    pub(crate) resolves: Option<String>,
    pub(crate) by: Option<String>,
    pub(crate) session_id: String,
    pub(crate) counterpart: Option<String>,
    pub(crate) counterpart_name: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) file_topics: Vec<String>,
    pub(crate) scribed_through: Option<String>,
    pub(crate) private: bool,
}

impl Entry {
    /// Every `<SCR id>#L<n>` key naming this line, under the file's current
    /// id and every alias it has carried.
    fn keys(&self) -> Vec<String> {
        let mut ids = vec![self.scr_id.clone()];
        ids.extend(self.aliases.clone());
        if let Some(stem) = self
            .path
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".md"))
            && !ids.iter().any(|id| id == stem)
        {
            ids.push(stem.to_owned());
        }
        ids.iter()
            .map(|id| format!("{id}#L{}", self.line))
            .collect()
    }

    fn date(&self) -> Option<String> {
        valid_date(self.time.get(..10)?).then(|| self.time[..10].to_owned())
    }
}

pub(crate) fn valid_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        && text[5..7]
            .parse::<u32>()
            .is_ok_and(|m| (1..=12).contains(&m))
        && text[8..10]
            .parse::<u32>()
            .is_ok_and(|d| (1..=31).contains(&d))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

pub(crate) fn optional_text(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.trim().to_owned())),
        Some(Value::String(_)) => Ok(None),
        Some(_) => Err(invalid(format!("scribe-query {key} must be a string"))),
    }
}

pub(crate) fn optional_date(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    let Some(value) = optional_text(input, key)? else {
        return Ok(None);
    };
    // A date-time is accepted and narrowed to its local date part.
    let date = value.get(..10).unwrap_or(&value);
    if valid_date(date) && (value.len() == 10 || value.as_bytes().get(10) == Some(&b'T')) {
        Ok(Some(date.to_owned()))
    } else {
        Err(invalid(format!(
            "scribe-query {key} must be a local date (YYYY-MM-DD)"
        )))
    }
}

fn ref_filter(input: &Map<String, Value>) -> Result<Vec<String>> {
    match input.get("refs") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(value)) => Ok(value
            .split(',')
            .map(|part| part.trim().to_owned())
            .filter(|part| !part.is_empty())
            .collect()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("scribe-query refs must be strings"))
            })
            .collect(),
        Some(_) => Err(invalid(
            "scribe-query refs must be a string or an array of strings",
        )),
    }
}

/// The closing quote of a quoted span starting at `start` (the byte of the
/// opening quote): the first quote followed by a known trailer key or the
/// end of the line.
fn closing_quote(text: &str, start: usize) -> Option<usize> {
    const TRAILERS: &[&str] = &[
        " why: ",
        " refs: ",
        " due: ",
        " matter: ",
        " review_by: ",
        " supersedes: ",
        " resolves: ",
        " by: ",
    ];
    let bytes = text.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'"'
            && (index + 1 == bytes.len()
                || TRAILERS.iter().any(|t| text[index + 1..].starts_with(t)))
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

pub(crate) fn parse_entry(file: &ScrFile, line_number: usize, line: &str) -> Option<Entry> {
    let rest = line.strip_prefix("- ")?;
    let mut parts = rest.splitn(3, ' ');
    let msg = parts.next()?.to_owned();
    let time = parts.next()?.to_owned();
    if time.len() < 10 || !valid_date(&time[..10]) {
        return None;
    }
    let after_time = parts.next()?;
    // Tag tokens: the first `#` token is the tag, the rest are topics.
    let mut tags: Vec<String> = Vec::new();
    let mut rest_offset = 0usize;
    for token in after_time.split(' ') {
        if let Some(tag) = token.strip_prefix('#') {
            tags.push(tag.to_owned());
            rest_offset += token.len() + 1;
        } else {
            break;
        }
    }
    let (tag, topics, no_why) = {
        let mut iter = tags.into_iter();
        let tag = iter.next()?;
        let mut topics = Vec::new();
        let mut no_why = false;
        for topic in iter {
            if topic == "no-why" {
                no_why = true;
            } else {
                topics.push(topic);
            }
        }
        (tag, topics, no_why)
    };
    let remainder = after_time.get(rest_offset..).unwrap_or_default();
    if !remainder.starts_with('"') {
        return None;
    }
    let close = closing_quote(remainder, 0)?;
    let quote = remainder[1..close].to_owned();
    let trailer = remainder[close + 1..].trim_start();
    let mut entry = Entry {
        scr_id: file.front.id.clone(),
        aliases: file.front.aliases.clone(),
        path: file.path.clone(),
        line: line_number,
        text: line.to_owned(),
        msg,
        time,
        tag,
        topics,
        no_why,
        quote,
        why_source: None,
        why_quote: None,
        refs: vec![],
        due: None,
        matter: None,
        review_by: None,
        supersedes: None,
        resolves: None,
        by: None,
        session_id: file.front.session_id.clone(),
        counterpart: file.front.counterpart.clone(),
        counterpart_name: file.front.counterpart_name.clone(),
        project: file.front.project.clone(),
        file_topics: file.front.topics.clone(),
        scribed_through: file.front.scribed_through.clone(),
        private: file.markdown.lines().take(30).any(|front_line| {
            front_line
                .trim()
                .strip_prefix("sensitivity:")
                .map(|value| value.trim().trim_matches('"').trim())
                == Some("private")
        }),
    };
    parse_trailer(trailer, &mut entry);
    Some(entry)
}

fn parse_trailer(trailer: &str, entry: &mut Entry) {
    let mut rest = trailer;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("why: ") {
            let Some(space) = tail.find(' ') else { break };
            entry.why_source = Some(tail[..space].to_owned());
            let quoted = &tail[space + 1..];
            if quoted.starts_with('"')
                && let Some(close) = closing_quote(quoted, 0)
            {
                entry.why_quote = Some(quoted[1..close].to_owned());
                rest = quoted[close + 1..].trim_start();
            } else {
                break;
            }
        } else if let Some(tail) = rest.strip_prefix("refs: ") {
            let end = trailer_value_end(tail);
            entry.refs = tail[..end]
                .split(',')
                .map(|part| part.trim().to_owned())
                .filter(|part| !part.is_empty())
                .collect();
            rest = tail[end..].trim_start();
        } else if let Some((key, tail)) = [
            ("due: ", 0usize),
            ("matter: ", 1),
            ("review_by: ", 2),
            ("supersedes: ", 3),
            ("resolves: ", 4),
            ("by: ", 5),
        ]
        .iter()
        .find_map(|(prefix, slot)| rest.strip_prefix(prefix).map(|tail| (*slot, tail)))
        {
            let end = tail.find(' ').unwrap_or(tail.len());
            let value = tail[..end].to_owned();
            match key {
                0 => entry.due = Some(value),
                1 => entry.matter = Some(value),
                2 => entry.review_by = Some(value),
                3 => entry.supersedes = Some(value),
                4 => entry.resolves = Some(value),
                _ => entry.by = Some(value),
            }
            rest = tail[end..].trim_start();
        } else {
            break;
        }
    }
}

fn trailer_value_end(text: &str) -> usize {
    const KEYS: &[&str] = &[
        " why: ",
        " refs: ",
        " due: ",
        " matter: ",
        " review_by: ",
        " supersedes: ",
        " resolves: ",
        " by: ",
    ];
    KEYS.iter()
        .filter_map(|key| text.find(key))
        .min()
        .unwrap_or(text.len())
}

pub(crate) fn entries_of(files: &[ScrFile]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for file in files {
        for (index, line) in file.markdown.lines().enumerate() {
            if let Some(entry) = parse_entry(file, index + 1, line) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by(|a, b| {
        a.time
            .cmp(&b.time)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    entries
}

fn run(call: &Call<'_>) -> Result<Value> {
    let input = call.input;
    let mode = optional_text(input, "mode")?;
    if let Some(mode) = &mode
        && !MODES.contains(&mode.as_str())
    {
        return Err(invalid(format!(
            "scribe-query mode must be one of: {}",
            MODES.join(", ")
        )));
    }
    let tag_filter = optional_text(input, "tag")?.map(|tag| tag.trim_start_matches('#').to_owned());
    let matter_filter = optional_text(input, "matter")?;
    let topic_filter =
        optional_text(input, "topic")?.map(|topic| topic.trim_start_matches('#').to_owned());
    let by_filter = optional_text(input, "by")?;
    let session_filter = optional_text(input, "session_id")?;
    let refs_filter = ref_filter(input)?;
    let mut since = optional_date(input, "since")?;
    let mut until = optional_date(input, "until")?;
    if let (Some(since_value), Some(until_value)) = (&since, &until)
        && since_value > until_value
    {
        return Err(invalid("scribe-query since must not be after until"));
    }
    let limit = match input.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_LIMIT).contains(limit))
            .ok_or_else(|| invalid(format!("scribe-query limit must be from 1 to {MAX_LIMIT}")))?,
    };

    let zone = call.ports.clock.time_zone();
    let zone_name = zone.clone().unwrap_or_else(|| "UTC".to_owned());
    let today = {
        let now_utc = format_rfc3339_utc(call.ports.clock.now());
        scribe::local(&now_utc, zone.as_deref()).map_or_else(
            |_| now_utc[..10].to_owned(),
            |local| local.format("%Y-%m-%d").to_string(),
        )
    };
    // The canned `landed` query is "what landed today?" when no range is given.
    if mode.as_deref() == Some("landed") && since.is_none() && until.is_none() {
        since = Some(today.clone());
        until = Some(today.clone());
    }

    let files = scribe::scr_files(&call.ports.library.scribe_files()?)?;
    let file_count = files.len();
    let entries = entries_of(&files);

    // Link resolution, over every entry regardless of the filters.
    let mut resolved_targets: Vec<String> = Vec::new();
    let mut superseded_targets: Vec<String> = Vec::new();
    // `<SCR id>#L<n>` of a decision -> the reason a `#why` note gives it.
    let mut why_by_target: BTreeMap<String, &Entry> = BTreeMap::new();
    for entry in &entries {
        if entry.tag == "resolved"
            && let Some(target) = &entry.resolves
        {
            resolved_targets.push(target.clone());
        }
        if entry.tag == "preference"
            && let Some(target) = &entry.supersedes
        {
            superseded_targets.push(target.clone());
        }
        if entry.tag == "why"
            && let Some(target) = &entry.resolves
        {
            why_by_target.insert(target.clone(), entry);
        }
    }
    let is_resolved = |entry: &Entry| {
        entry
            .keys()
            .iter()
            .any(|key| resolved_targets.contains(key))
    };
    let is_superseded = |entry: &Entry| {
        entry
            .keys()
            .iter()
            .any(|key| superseded_targets.contains(key))
    };
    let filled_why = |entry: &Entry| -> Option<&Entry> {
        entry
            .keys()
            .iter()
            .find_map(|key| why_by_target.get(key).copied())
    };

    let effective_tag: Option<&str> = match mode.as_deref() {
        Some("pending") => Some("pending"),
        Some("pref") => Some("preference"),
        Some("landed") => Some("landed"),
        _ => tag_filter.as_deref(),
    };

    let mut results: Vec<&Entry> = entries
        .iter()
        .filter(|entry| {
            if let Some(tag) = effective_tag
                && entry.tag != tag
            {
                return false;
            }
            if mode.as_deref() == Some("why")
                && !matches!(entry.tag.as_str(), "decision" | "agent-decision")
            {
                return false;
            }
            // A mode's own tag wins; an explicit different tag narrows nothing
            // further, so only apply the caller's tag in the general mode.
            if mode.is_none()
                && let Some(tag) = &tag_filter
                && entry.tag != *tag
            {
                return false;
            }
            if let Some(matter) = &matter_filter {
                let matches = entry.matter.as_deref() == Some(matter.as_str())
                    || (mode.as_deref() == Some("why") && entry.topics.contains(matter));
                if !matches {
                    return false;
                }
            }
            if let Some(topic) = &topic_filter
                && !entry.topics.contains(topic)
            {
                return false;
            }
            if let Some(by) = &by_filter
                && entry.by.as_deref() != Some(by.as_str())
            {
                return false;
            }
            if let Some(session) = &session_filter
                && entry.session_id != *session
            {
                return false;
            }
            if !refs_filter.iter().all(|want| entry.refs.contains(want)) {
                return false;
            }
            if let Some(date) = entry.date() {
                if since.as_ref().is_some_and(|since| date < *since)
                    || until.as_ref().is_some_and(|until| date > *until)
                {
                    return false;
                }
            } else if since.is_some() || until.is_some() {
                return false;
            }
            // Link resolution: an open pending, a current preference.
            if entry.tag == "pending" && is_resolved(entry) {
                return false;
            }
            if entry.tag == "preference" && is_superseded(entry) {
                return false;
            }
            true
        })
        .collect();

    if mode.as_deref() == Some("pref") {
        // Current means the latest surviving preference per matter.
        let mut latest: BTreeMap<String, &Entry> = BTreeMap::new();
        for entry in &results {
            let key = entry.matter.clone().unwrap_or_default();
            latest.insert(key, entry);
        }
        let mut kept: Vec<&Entry> = latest.into_values().collect();
        kept.sort_by(|a, b| {
            a.time
                .cmp(&b.time)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
        });
        results = kept;
    }

    let total = results.len();
    results.truncate(limit);

    let rendered: Vec<Value> = results
        .iter()
        .map(|entry| {
            let why_note = filled_why(entry);
            let reason = entry
                .why_quote
                .clone()
                .or_else(|| why_note.map(|note| note.quote.clone()));
            let stale = entry.tag == "preference"
                && entry.review_by.as_deref().is_some_and(|review_by| {
                    valid_date(&review_by[..10.min(review_by.len())]) && review_by[..10] < today[..]
                });
            let resume = if entry.session_id.is_empty() {
                Value::Null
            } else {
                json!(format!("claude --resume {}", entry.session_id))
            };
            json!({
                "text": entry.text,
                "line_text": entry.text,
                "quote": entry.quote,
                "id": entry.scr_id,
                "scr_id": entry.scr_id,
                "path": entry.path,
                "line": entry.line,
                "file_line": format!("{}:{}", entry.path, entry.line),
                "citation": format!("{}#L{}", entry.scr_id, entry.line),
                "msg": entry.msg,
                "message_id": entry.msg,
                "time": entry.time,
                "at": entry.time,
                "tag": entry.tag,
                "topics": entry.topics,
                "refs": entry.refs,
                "by": entry.by,
                "matter": entry.matter,
                "review_by": entry.review_by,
                "stale": stale,
                "due": entry.due,
                "resolves": entry.resolves,
                "supersedes": entry.supersedes,
                "session_id": entry.session_id,
                "counterpart": entry.counterpart,
                "counterpart_name": entry.counterpart_name,
                "resume": resume,
                "resume_command": resume,
                "why": reason,
                "reason": reason,
                "why_source": entry.why_source,
                "why_filled_by": why_note.map(|note| format!("{}#L{}", note.scr_id, note.line)),
                "no_why": entry.no_why && entry.why_quote.is_none() && why_note.is_none(),
            })
        })
        .collect();

    Ok(json!({
        "mode": mode,
        "results": rendered,
        "count": total,
        "files_searched": file_count,
        "file_count": file_count,
        "since": since,
        "until": until,
        "date_range": {"since": since, "until": until},
        "time_zone": zone_name,
        "today": today,
    }))
}
