//! Scribe v1 (specs/scribe.md): the parsed message file, the rules page
//! sections, the SCR file's name, front matter and entry lines, and the
//! model-free validation `office.scribe-append` applies. Pure: callers
//! read and write through the Library port.

use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{Rules, Session, scrub};
use crate::ports::LibraryText;
use crate::{Error, Result};

/// Workspace-relative directory of the parsed message files.
pub const PARSE_DIR: &str = ".orbit/tmp/scribe";
/// Earlier messages returned as read-only context.
pub const CONTEXT_MESSAGES: usize = 10;
/// Longest title slug in an SCR file name.
pub const SLUG_MAX: usize = 48;
/// Session-id characters appended to a clashing SCR name.
pub const CLASH_SUFFIX: usize = 6;
/// Library directory of the SCR files.
pub const SCR_DIR: &str = "sources/conversation";
/// The one transcript format the conversation adapter reads.
pub const PROVIDER: &str = "claude-code";
/// Longest note text, in bytes.
pub const NOTE_MAX: usize = 4096;

const USER_TAGS: &[&str] = &["decision", "direction", "todo", "question", "reminder"];
const AGENT_TAGS: &[&str] = &["agent-decision", "pending", "resolved", "incident"];
const COUNTERPARTS: &[&str] = &["orchestrator", "agent", "coworker", "meeting", "self"];

fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

// ---- messages ---------------------------------------------------------

/// One message exchanged between the user and the agent, as the scribe
/// reads it: the kept text of one transcript record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// Stable id: the nth kept message is always `m-<n>` (from `m-0001`).
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    /// The record's RFC 3339 timestamp.
    pub at: String,
    /// The record's kept, scrubbed text lines joined by newlines.
    pub text: String,
}

/// The id of the message at zero-based `index`.
#[must_use]
pub fn message_id(index: usize) -> String {
    format!("m-{:04}", index.saturating_add(1))
}

/// The zero-based index a canonical message id names.
#[must_use]
pub fn message_index(id: &str) -> Option<usize> {
    let digits = id.strip_prefix("m-")?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let index = digits.parse::<usize>().ok()?.checked_sub(1)?;
    (message_id(index) == id).then_some(index)
}

/// Group the adapter's text lines into messages. Lines of one transcript
/// record share a prefix digest, so appending records only adds messages.
#[must_use]
pub fn messages(session: &Session) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut last: Option<&str> = None;
    for (line, prefix) in session.lines.iter().zip(&session.line_prefix_sha256) {
        match out.last_mut() {
            Some(message) if last == Some(prefix.as_str()) => {
                message.text.push('\n');
                message.text.push_str(&line.text);
            }
            _ => out.push(Message {
                id: message_id(out.len()),
                role: line.role.clone(),
                at: line.timestamp.clone(),
                text: line.text.clone(),
            }),
        }
        last = Some(prefix);
    }
    out
}

/// One JSON object per line, in id order.
pub fn to_jsonl(messages: &[Message]) -> Result<String> {
    let mut out = String::new();
    for message in messages {
        out.push_str(
            &serde_json::to_string(message).map_err(|_| invalid("unserializable message"))?,
        );
        out.push('\n');
    }
    Ok(out)
}

/// Read a parsed message file; ids must run `m-0001` upward.
pub fn from_jsonl(text: &str) -> Result<Vec<Message>> {
    let damaged = || {
        refusal(
            "scribe_parse_invalid",
            "the parsed message file is damaged; run office.scribe-pending again",
        )
    };
    let mut out = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let message: Message = serde_json::from_str(line).map_err(|_| damaged())?;
        if message.id != message_id(out.len()) || !["user", "assistant"].contains(&&*message.role) {
            return Err(damaged());
        }
        out.push(message);
    }
    Ok(out)
}

/// Session facts `office.scribe-pending` records beside the message file,
/// so `office.scribe-append` can name and describe the SCR file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parsed {
    /// Provider session id.
    pub session_id: String,
    /// Transcript format, `claude-code`.
    pub provider: String,
    /// Last model the transcript reports.
    pub model: Option<String>,
    /// Host the session was parsed on.
    pub host: Option<String>,
    /// Project name: the last component of `cwd`.
    pub project: Option<String>,
    /// The session's working directory.
    pub cwd: Option<String>,
    /// Git branch at session start.
    pub branch: Option<String>,
    /// Transcript path as given.
    pub transcript: String,
    /// Workspace-relative path of the message file.
    pub parsed: String,
    /// First conversational record's timestamp.
    pub started_at: Option<String>,
}

/// Workspace-relative path of a session's message file.
#[must_use]
pub fn parse_path(session_id: &str) -> String {
    format!("{PARSE_DIR}/{session_id}.jsonl")
}

/// The last path component of a working directory, as a project name.
#[must_use]
pub fn project_of(cwd: &str) -> Option<String> {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

// ---- time -------------------------------------------------------------

/// Parse RFC 3339 and convert to the host zone (UTC when unknown).
pub fn local(at: &str, zone: Option<&str>) -> Result<DateTime<FixedOffset>> {
    let instant = DateTime::parse_from_rfc3339(at)
        .map_err(|_| refusal("scribe_parse_invalid", "message time is not RFC 3339"))?;
    Ok(
        match zone.and_then(|name| name.parse::<chrono_tz::Tz>().ok()) {
            Some(tz) => instant.with_timezone(&tz).fixed_offset(),
            None => instant.with_timezone(&chrono::Utc).fixed_offset(),
        },
    )
}

/// Minute-precision local time, as written on entry lines.
#[must_use]
pub fn line_time(at: DateTime<FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M%:z").to_string()
}

fn valid_due(due: &str) -> bool {
    DateTime::parse_from_rfc3339(due).is_ok()
        || DateTime::parse_from_str(due, "%Y-%m-%dT%H:%M%:z").is_ok()
        || NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok()
}

// ---- rules ------------------------------------------------------------

/// Who may take a tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Authors {
    /// Daniel's own messages and `by: user` notes.
    User,
    /// Assistant messages and agent notes, with `by: <crew>`.
    Agent,
    /// Either.
    Any,
}

/// One allowed tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// The tag, without `#`.
    pub name: String,
    /// One-line meaning.
    pub meaning: String,
    /// Examples from the rules page.
    pub examples: Vec<String>,
    /// Who may take it; fixed for the built-in tags.
    pub authors: Authors,
}

/// The rules the scribe agent follows and append enforces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScribeRules {
    /// `page` when `dom-scribe-rules` supplied any section, else `defaults`.
    pub source: &'static str,
    /// Allowed tags.
    pub tags: Vec<Tag>,
    /// Allowed topics.
    pub topics: Vec<String>,
    /// Capture guidance for the agent.
    pub capture: Vec<String>,
}

fn authors_of(name: &str) -> Authors {
    if USER_TAGS.contains(&name) {
        Authors::User
    } else if AGENT_TAGS.contains(&name) {
        Authors::Agent
    } else {
        Authors::Any
    }
}

fn default_tags() -> Vec<Tag> {
    [
        ("decision", "Daniel settles a choice."),
        ("direction", "A standing instruction from Daniel."),
        ("question", "An open question for later."),
        ("todo", "An action, optionally with due."),
        ("reminder", "A time-bound note to self."),
        (
            "fact",
            "A stated fact about hosts, people or configuration.",
        ),
        (
            "landed",
            "A PR merged or a task done, reported in the chat.",
        ),
        (
            "agent-decision",
            "An orchestrator settles a choice within its delegated scope.",
        ),
        ("pending", "Work an orchestrator started or is waiting on."),
        (
            "resolved",
            "Closes an earlier pending line, named by resolves: <SCR id>#L<n>.",
        ),
        ("incident", "A failure or broken run an orchestrator hit."),
    ]
    .into_iter()
    .map(|(name, meaning)| Tag {
        name: name.into(),
        meaning: meaning.into(),
        examples: vec![],
        authors: authors_of(name),
    })
    .collect()
}

fn default_capture() -> Vec<String> {
    [
        "Only Daniel's messages and by: user notes take decision, direction, todo, question and reminder.",
        "Assistant messages take landed and fact, and the agent tags agent-decision, pending, resolved and incident with by: <crew>.",
        "Skip status chatter and restated plans.",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn token(text: &str) -> Option<String> {
    let name = text.trim().trim_matches(['`', '*', '#']).trim();
    (!name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b)))
    .then(|| name.to_owned())
}

/// Topic token from a project name: lowercase, other characters as `-`.
#[must_use]
pub fn topic_of(project: &str) -> Option<String> {
    let slug = slug(project);
    (!slug.is_empty()).then_some(slug)
}

/// The rules from the page's `Tags`, `Topics` and `Capture` sections. A
/// missing page, a private page or a missing section uses the defaults;
/// default topics are the project name.
#[must_use]
pub fn scribe_rules(page: Option<&Rules>, project: Option<&str>) -> ScribeRules {
    let mut rules = ScribeRules {
        source: "defaults",
        tags: default_tags(),
        topics: project.and_then(topic_of).into_iter().collect(),
        capture: default_capture(),
    };
    let Some(body) = page.and_then(|page| page.body.as_deref()) else {
        return rules;
    };
    let mut section = String::new();
    let (mut tags, mut topics, mut capture) = (Vec::<Tag>::new(), Vec::new(), Vec::new());
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix('#') {
            section = heading
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .to_ascii_lowercase();
            continue;
        }
        let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };
        let nested = line.starts_with([' ', '\t']);
        match section.as_str() {
            "tags" if nested => {
                if let Some(tag) = tags.last_mut() {
                    tag.examples.push(item.trim().to_owned());
                }
            }
            "tags" => {
                let (name, meaning) = item
                    .split_once(':')
                    .or_else(|| item.split_once(" — "))
                    .or_else(|| item.split_once(" - "))
                    .unwrap_or((item, ""));
                if let Some(name) = token(name)
                    && !tags.iter().any(|tag| tag.name == name)
                {
                    tags.push(Tag {
                        authors: authors_of(&name),
                        name,
                        meaning: meaning.trim().to_owned(),
                        examples: vec![],
                    });
                }
            }
            "topics" if !nested => {
                let name = item.split_once(':').map_or(item, |(name, _)| name);
                if let Some(name) = token(name)
                    && !topics.contains(&name)
                {
                    topics.push(name);
                }
            }
            "capture" if !nested => capture.push(item.trim().to_owned()),
            _ => {}
        }
    }
    if !tags.is_empty() {
        rules.tags = tags;
        rules.source = "page";
    }
    if !topics.is_empty() {
        rules.topics = topics;
        rules.source = "page";
    }
    if !capture.is_empty() {
        rules.capture = capture;
        rules.source = "page";
    }
    rules
}

// ---- SCR naming -------------------------------------------------------

/// Lowercase ASCII letters and digits, other runs as one `-`, at most
/// [`SLUG_MAX`] characters.
#[must_use]
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= SLUG_MAX {
            break;
        }
    }
    out.truncate(SLUG_MAX);
    out.trim_end_matches('-').to_owned()
}

/// `SCR-<dd-hhmm>-<slug>` for a session starting at `start` (local).
#[must_use]
pub fn scr_id(start: DateTime<FixedOffset>, slug: &str) -> String {
    let slug = if slug.is_empty() { "session" } else { slug };
    format!("SCR-{}-{slug}", start.format("%d-%H%M"))
}

/// `sources/conversation/<yyyy>/<mm>/<id>.md`.
#[must_use]
pub fn scr_path(start: DateTime<FixedOffset>, id: &str) -> String {
    format!("{SCR_DIR}/{}/{id}.md", start.format("%Y/%m"))
}

/// `id` with the clash suffix: the session id's first characters.
#[must_use]
pub fn with_suffix(id: &str, session_id: &str) -> String {
    let suffix: String = session_id
        .chars()
        .take(CLASH_SUFFIX)
        .map(|c| match c {
            '_' => '-',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    format!("{id}-{suffix}")
}

fn valid_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("SCR-") else {
        return false;
    };
    let bytes = rest.as_bytes();
    bytes.len() > 8
        && bytes.len() <= 8 + SLUG_MAX + 1 + CLASH_SUFFIX
        && bytes[..2].iter().all(u8::is_ascii_digit)
        && bytes[2] == b'-'
        && bytes[3..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !rest.ends_with('-')
}

/// Refuse anything but `sources/conversation/<yyyy>/<mm>/SCR-….md`.
pub fn validate_scr_path(relative: &Path) -> Result<()> {
    let text = relative.to_str().unwrap_or_default();
    let parts: Vec<&str> = text.split('/').collect();
    let ok = parts.len() == 5
        && parts[0] == "sources"
        && parts[1] == "conversation"
        && parts[2].len() == 4
        && parts[2].bytes().all(|b| b.is_ascii_digit())
        && parts[3].len() == 2
        && parts[3].bytes().all(|b| b.is_ascii_digit())
        && parts[4].strip_suffix(".md").is_some_and(valid_id);
    if ok {
        Ok(())
    } else {
        Err(refusal(
            "scribe_path_invalid",
            "SCR files live at sources/conversation/<yyyy>/<mm>/SCR-<dd-hhmm>-<slug>.md",
        ))
    }
}

/// True for a Library path that names an SCR file.
#[must_use]
pub fn is_scr_path(relative: &str) -> bool {
    validate_scr_path(Path::new(relative)).is_ok()
}

// ---- the SCR file -----------------------------------------------------

/// SCR front matter, every key always present and on one line, so file
/// line numbers of entries never move.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Front {
    /// File stem.
    pub id: String,
    /// Title set by `/scribe title`; `None` until then.
    pub title: Option<String>,
    /// Provider session id.
    pub session_id: String,
    /// Transcript format.
    pub provider: String,
    /// Last model the transcript reports.
    pub model: Option<String>,
    /// orchestrator, agent, coworker, meeting or self.
    pub counterpart: Option<String>,
    /// Free-form name of the counterpart.
    pub counterpart_name: Option<String>,
    /// Host the session was parsed on.
    pub host: Option<String>,
    /// Project name.
    pub project: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Git branch at session start.
    pub branch: Option<String>,
    /// Transcript path.
    pub transcript: Option<String>,
    /// Message file path.
    pub parsed: Option<String>,
    /// Session start, host local time.
    pub started_at: Option<String>,
    /// Last append, host local time.
    pub last_scribed_at: Option<String>,
    /// Last message given to the scribe.
    pub scribed_through: Option<String>,
    /// Merged entry topics.
    pub topics: Vec<String>,
    /// Earlier ids of this file.
    pub aliases: Vec<String>,
}

fn text(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn list(fields: &Map<String, Value>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Front {
    /// Read the front matter of an SCR file.
    pub fn parse(markdown: &str) -> Result<Self> {
        let fields = crate::domain::metrics::metadata(markdown)?;
        Ok(Self {
            id: text(&fields, "id").unwrap_or_default(),
            title: text(&fields, "title"),
            session_id: text(&fields, "session_id").unwrap_or_default(),
            provider: text(&fields, "provider").unwrap_or_default(),
            model: text(&fields, "model"),
            counterpart: text(&fields, "counterpart"),
            counterpart_name: text(&fields, "counterpart_name"),
            host: text(&fields, "host"),
            project: text(&fields, "project"),
            cwd: text(&fields, "cwd"),
            branch: text(&fields, "branch"),
            transcript: text(&fields, "transcript"),
            parsed: text(&fields, "parsed"),
            started_at: text(&fields, "started_at"),
            last_scribed_at: text(&fields, "last_scribed_at"),
            scribed_through: text(&fields, "scribed_through"),
            topics: list(&fields, "topics"),
            aliases: list(&fields, "aliases"),
        })
    }

    /// Render the front matter block, `---` lines included.
    #[must_use]
    pub fn render(&self) -> String {
        use crate::domain::knowledge::frontmatter::flow_value;
        let opt = |value: &Option<String>| value.as_ref().map_or(Value::Null, |v| json!(v));
        let fields: [(&str, Value); 18] = [
            ("id", json!(self.id)),
            ("title", opt(&self.title)),
            ("session_id", json!(self.session_id)),
            ("provider", json!(self.provider)),
            ("model", opt(&self.model)),
            ("counterpart", opt(&self.counterpart)),
            ("counterpart_name", opt(&self.counterpart_name)),
            ("host", opt(&self.host)),
            ("project", opt(&self.project)),
            ("cwd", opt(&self.cwd)),
            ("branch", opt(&self.branch)),
            ("transcript", opt(&self.transcript)),
            ("parsed", opt(&self.parsed)),
            ("started_at", opt(&self.started_at)),
            ("last_scribed_at", opt(&self.last_scribed_at)),
            ("scribed_through", opt(&self.scribed_through)),
            ("topics", json!(self.topics)),
            ("aliases", json!(self.aliases)),
        ];
        let mut out = String::from("---\n");
        for (key, value) in fields {
            out.push_str(&format!("{key}: {}\n", flow_value(&value)));
        }
        out.push_str("---\n");
        out
    }

    /// Metadata a caller reports about this file.
    #[must_use]
    pub fn view(&self, path: &str, exists: bool) -> Value {
        json!({"exists":exists,"id":self.id,"path":path,"title":self.title,
            "scribed_through":self.scribed_through,"last_scribed_at":self.last_scribed_at,
            "topics":self.topics,"aliases":self.aliases,"counterpart":self.counterpart,
            "counterpart_name":self.counterpart_name})
    }
}

/// An SCR file read from the Library.
#[derive(Clone, Debug)]
pub struct ScrFile {
    /// Library-relative path.
    pub path: String,
    /// Full Markdown.
    pub markdown: String,
    /// Parsed front matter.
    pub front: Front,
}

/// Every readable SCR file, parsed.
pub fn scr_files(files: &[LibraryText]) -> Result<Vec<ScrFile>> {
    files
        .iter()
        .filter(|file| is_scr_path(&file.path))
        .map(|file| {
            let markdown = file.markdown.clone().ok_or_else(|| {
                refusal(
                    "library_document_unreadable",
                    format!("cannot read {}", file.path),
                )
            })?;
            Ok(ScrFile {
                path: file.path.clone(),
                front: Front::parse(&markdown)?,
                markdown,
            })
        })
        .collect()
}

/// The SCR file recording `session_id`, if any.
pub fn find_session<'a>(files: &'a [ScrFile], session_id: &str) -> Result<Option<&'a ScrFile>> {
    let mut found = files.iter().filter(|f| f.front.session_id == session_id);
    let first = found.next();
    if found.next().is_some() {
        return Err(refusal(
            "scribe_session_conflict",
            "several SCR files record this session id; reconcile them first",
        ));
    }
    Ok(first)
}

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

/// The slug source of an untitled session: the user's first message.
#[must_use]
pub fn first_words(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user")
        .or_else(|| messages.first())
        .and_then(|m| m.text.lines().next())
        .map(slug)
        .unwrap_or_default()
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
    let slug = title.map_or_else(|| first_words(messages), slug);
    let id = free_id(files, &scr_id(start, &slug), &parsed.session_id)?;
    let path = scr_path(start, &id);
    Ok((id, path))
}

// ---- entries ----------------------------------------------------------

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
                    "each entry is {msg, quote} or {text}, with tag and optional topics, refs, due, resolves and by; no other keys",
                )
            })?;
            match (&entry.msg, &entry.quote, &entry.text) {
                (Some(_), Some(_), None) | (None, None, Some(_)) => Ok(entry),
                _ => Err(invalid(
                    "an entry cites a message with msg and quote, or is a note with text alone",
                )),
            }
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

/// What [`append`] decided.
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
    /// Last message given to the agent.
    pub through: &'a str,
    /// New title, renaming the file.
    pub title: Option<&'a str>,
    /// Counterpart kind.
    pub counterpart: Option<&'a str>,
    /// Counterpart name.
    pub counterpart_name: Option<&'a str>,
    /// Current time, RFC 3339.
    pub now: &'a str,
    /// Host time zone (IANA), UTC when `None`.
    pub zone: Option<&'a str>,
}

fn single_line(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn valid_crew(by: &str) -> bool {
    by != "user"
        && by.len() <= 64
        && by.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && by
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}

fn valid_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"#-_.:/@".contains(&b))
}

fn escape(text: &str) -> String {
    text.replace("\r\n", "\\n").replace(['\n', '\r'], "\\n")
}

/// The entry line, without the time token, for duplicate detection.
fn identity(line: &str) -> String {
    line.split(' ')
        .enumerate()
        .filter(|(index, _)| *index != 2)
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether line `n` (1-based) of `markdown` is a `#pending` entry.
fn pending_at(markdown: &str, n: usize) -> bool {
    n.checked_sub(1)
        .and_then(|index| markdown.lines().nth(index))
        .is_some_and(|line| {
            let mut parts = line.split(' ');
            parts.next() == Some("-") && parts.nth(2) == Some("#pending")
        })
}

fn split_body(markdown: &str) -> String {
    super::super::source_body(markdown)
}

struct Ctx<'a> {
    messages: &'a [Message],
    rules: &'a ScribeRules,
    files: &'a [ScrFile],
    after: Option<usize>,
    through: usize,
    zone: Option<&'a str>,
    now: DateTime<FixedOffset>,
}

impl Ctx<'_> {
    /// Render one entry or say why it is refused. `own` is this file's
    /// planned Markdown so far, with its current and earlier ids.
    fn line(
        &self,
        entry: &Entry,
        own: &str,
        own_ids: &[String],
    ) -> std::result::Result<String, (&'static str, String)> {
        let fail = |reason: &'static str, message: &str| Err((reason, message.to_owned()));
        // The cited message and its author.
        let (head, at, quote, user) = if let Some(msg) = &entry.msg {
            let Some(index) = message_index(msg).filter(|i| *i < self.messages.len()) else {
                return fail("unknown_message", "no parsed message has this id");
            };
            if self.after.is_some_and(|after| index <= after) {
                return fail(
                    "context_message",
                    "the message is at or before scribed_through; context is read-only",
                );
            }
            if index > self.through {
                return fail("after_through", "the message is after through");
            }
            let message = &self.messages[index];
            let user = message.role == "user";
            match entry.by.as_deref() {
                Some("user") if !user => {
                    return fail("author_mismatch", "by: user cites an assistant message");
                }
                Some(_) if user => {
                    return fail("author_mismatch", "a crew by cites a user message");
                }
                _ => {}
            }
            let quote = entry.quote.as_deref().unwrap_or_default();
            if quote.is_empty() || !message.text.contains(quote) {
                return fail(
                    "quote_not_verbatim",
                    "the quote is not an exact substring of the cited message",
                );
            }
            let at = local(&message.at, self.zone).map_err(|_| {
                (
                    "unknown_message",
                    "the cited message has no valid time".to_owned(),
                )
            })?;
            (msg.clone(), at, quote.to_owned(), user)
        } else {
            let note = entry.text.as_deref().unwrap_or_default();
            if note.trim().is_empty() || note.len() > NOTE_MAX {
                return fail("invalid_note", "a note needs 1 to 4096 bytes of text");
            }
            let user = match entry.by.as_deref() {
                Some("user") => true,
                Some(_) => false,
                None => return fail("missing_by", "a note needs by: user or by: <crew>"),
            };
            ("note".to_owned(), self.now, scrub(note), user)
        };
        if let Some(by) = entry.by.as_deref()
            && by != "user"
            && !valid_crew(by)
        {
            return fail("invalid_by", "by is user or a crew name");
        }
        let Some(tag) = self.rules.tags.iter().find(|tag| tag.name == entry.tag) else {
            return fail("unknown_tag", "the tag is not in the scribe rules");
        };
        let mut topics: Vec<&str> = Vec::new();
        for topic in &entry.topics {
            if !self.rules.topics.contains(topic) {
                return fail("unknown_topic", "a topic is not in the scribe rules");
            }
            if !topics.contains(&topic.as_str()) {
                topics.push(topic);
            }
        }
        match tag.authors {
            Authors::User if !user => {
                return fail(
                    "tag_not_permitted",
                    "this tag is only for the user's messages and by: user notes",
                );
            }
            Authors::Agent if user => {
                return fail(
                    "tag_not_permitted",
                    "this tag is only for assistant messages and agent notes",
                );
            }
            Authors::Agent if entry.by.is_none() => {
                return fail("missing_by", "an agent tag needs by: <crew>");
            }
            Authors::User | Authors::Agent | Authors::Any => {}
        }
        if entry.refs.iter().any(|r| !valid_ref(r)) {
            return fail(
                "invalid_ref",
                "refs are single tokens such as ORB-1 or PR#2",
            );
        }
        if entry.due.as_deref().is_some_and(|due| !valid_due(due)) {
            return fail(
                "invalid_due",
                "due is an RFC 3339 time or a YYYY-MM-DD date",
            );
        }
        match (tag.name.as_str(), entry.resolves.as_deref()) {
            ("resolved", None) => {
                return fail(
                    "missing_resolves",
                    "a resolved entry names its pending line",
                );
            }
            ("resolved", Some(target)) => {
                if !self.resolves(target, own, own_ids) {
                    return fail(
                        "unresolved_pending",
                        "resolves does not name an existing pending line",
                    );
                }
            }
            (_, Some(_)) => {
                return fail(
                    "resolves_not_allowed",
                    "only a resolved entry takes resolves",
                );
            }
            (_, None) => {}
        }
        let mut line = format!("- {head} {} #{}", line_time(at), tag.name);
        for topic in topics {
            line.push_str(&format!(" #{topic}"));
        }
        line.push_str(&format!(" \"{}\"", escape(&quote)));
        if !entry.refs.is_empty() {
            line.push_str(&format!(" refs: {}", entry.refs.join(", ")));
        }
        if let Some(due) = &entry.due {
            line.push_str(&format!(" due: {due}"));
        }
        if let Some(target) = &entry.resolves {
            line.push_str(&format!(" resolves: {target}"));
        }
        if let Some(by) = &entry.by {
            line.push_str(&format!(" by: {by}"));
        }
        Ok(line)
    }

    fn resolves(&self, target: &str, own: &str, own_ids: &[String]) -> bool {
        let Some((id, line)) = target.split_once("#L") else {
            return false;
        };
        let Some(n) = line
            .parse::<usize>()
            .ok()
            .filter(|_| line.bytes().all(|b| b.is_ascii_digit()))
        else {
            return false;
        };
        if own_ids.iter().any(|own_id| own_id == id) {
            return pending_at(own, n);
        }
        self.files
            .iter()
            .filter(|f| f.front.id == id || f.front.aliases.iter().any(|a| a == id))
            .any(|f| pending_at(&f.markdown, n))
    }
}

/// Validate entries and plan the SCR file after this append.
pub fn append(
    request: &Request<'_>,
    entries: &[Entry],
    messages: &[Message],
    parsed: &Parsed,
    rules: &ScribeRules,
    files: &[ScrFile],
) -> Result<Plan> {
    if let Some(title) = request.title
        && !single_line(title, 4096)
    {
        return Err(invalid("title must be a nonempty single line"));
    }
    if let Some(counterpart) = request.counterpart
        && !COUNTERPARTS.contains(&counterpart)
    {
        return Err(invalid(
            "counterpart is orchestrator, agent, coworker, meeting or self",
        ));
    }
    if let Some(name) = request.counterpart_name
        && !single_line(name, 256)
    {
        return Err(invalid(
            "counterpart_name must be a nonempty single line of at most 256 bytes",
        ));
    }
    let through = message_index(request.through)
        .filter(|i| *i < messages.len())
        .ok_or_else(|| {
            refusal(
                "scribe_through_unknown",
                "through must name a parsed message; run office.scribe-pending first",
            )
        })?;
    let existing = find_session(files, request.session_id)?;
    let previous_through = existing.and_then(|f| f.front.scribed_through.clone());
    let after = match &previous_through {
        None => None,
        Some(id) => Some(message_index(id).ok_or_else(|| {
            refusal(
                "scribe_file_invalid",
                "the SCR file's scribed_through is not a message id",
            )
        })?),
    };
    if after.is_some_and(|after| through < after) {
        return Err(refusal(
            "scribe_through_behind",
            "through is before the SCR file's scribed_through",
        ));
    }
    let now = local(request.now, request.zone)?;
    let others: Vec<ScrFile> = files
        .iter()
        .filter(|f| f.front.session_id != request.session_id)
        .cloned()
        .collect();

    // The file as it stands, or a new one.
    let (mut front, mut path, body, previous) = if let Some(file) = existing {
        (
            file.front.clone(),
            file.path.clone(),
            split_body(&file.markdown),
            Some((file.path.clone(), file.markdown.clone())),
        )
    } else {
        let (id, path) = planned(&others, parsed, messages, request.title, request.zone)?;
        let start = session_start(parsed, messages, request.zone)?;
        let front = Front {
            id,
            title: request.title.map(str::to_owned),
            session_id: request.session_id.to_owned(),
            provider: parsed.provider.clone(),
            model: parsed.model.clone(),
            host: parsed.host.clone(),
            project: parsed.project.clone(),
            cwd: parsed.cwd.clone(),
            branch: parsed.branch.clone(),
            transcript: Some(parsed.transcript.clone()),
            parsed: Some(parsed.parsed.clone()),
            started_at: Some(start.to_rfc3339()),
            ..Front::default()
        };
        (front, path, String::new(), None)
    };
    let created = previous.is_none();

    // A changed title renames the file once and keeps the old id.
    let mut renamed_from = None;
    if let Some(title) = request.title
        && !created
        && front.title.as_deref() != Some(title)
    {
        front.title = Some(title.to_owned());
        let prefix = front.id.get(..12).unwrap_or_default().to_owned();
        let candidate = free_id(
            &others,
            &format!("{prefix}{}", {
                let s = slug(title);
                if s.is_empty() { "session".into() } else { s }
            }),
            request.session_id,
        )?;
        if candidate != front.id {
            let dir = path
                .rsplit_once('/')
                .map(|(d, _)| d.to_owned())
                .unwrap_or_default();
            renamed_from = Some(front.id.clone());
            front.aliases.push(front.id.clone());
            front.id = candidate;
            path = format!("{dir}/{}.md", front.id);
        }
    }
    validate_scr_path(Path::new(&path))?;
    if let Some(counterpart) = request.counterpart {
        front.counterpart = Some(counterpart.to_owned());
    }
    if let Some(name) = request.counterpart_name {
        front.counterpart_name = Some(name.to_owned());
    }

    let ctx = Ctx {
        messages,
        rules,
        files: &others,
        after,
        through,
        zone: request.zone,
        now,
    };
    let header_lines = front.render().lines().count();
    let mut body = body;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    let mut known: BTreeSet<String> = body.lines().map(identity).collect();
    let mut added = Vec::new();
    let mut refused = Vec::new();
    let mut duplicates = 0;
    let mut own_ids = vec![front.id.clone()];
    own_ids.extend(front.aliases.iter().cloned());
    let mut topics = front.topics.clone();
    for (index, entry) in entries.iter().enumerate() {
        let own = format!("{}{body}", front.render());
        match ctx.line(entry, &own, &own_ids) {
            Ok(line) => {
                if !known.insert(identity(&line)) {
                    duplicates += 1;
                    continue;
                }
                for topic in &entry.topics {
                    if !topics.contains(topic) {
                        topics.push(topic.clone());
                    }
                }
                body.push_str(&line);
                body.push('\n');
                added.push((header_lines + body.lines().count(), line));
            }
            Err((reason, message)) => refused.push(Refusal {
                index,
                msg: entry.msg.clone().unwrap_or_else(|| "note".into()),
                reason,
                message,
            }),
        }
    }
    front.topics = topics;
    front.scribed_through = Some(message_id(through));
    let unchanged = |front: &Front| {
        previous
            .as_ref()
            .is_some_and(|(_, old)| format!("{}{body}", front.render()) == *old)
    };
    let markdown = if unchanged(&front) {
        None
    } else {
        front.last_scribed_at = Some(line_time(now));
        Some(format!("{}{body}", front.render()))
    };
    Ok(Plan {
        id: front.id.clone(),
        path,
        previous,
        markdown,
        added,
        duplicates,
        refused,
        renamed_from,
        previous_through,
        front,
    })
}
