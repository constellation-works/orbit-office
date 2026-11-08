//! Deterministic session continuity. All inputs are recorded data; nothing here
//! interprets a quoted entry as an instruction or performs I/O.

pub mod handoff;
pub mod listing;
pub mod scr;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::artifact::Artifact;
use super::knowledge::{Page, frontmatter};
use super::metrics::{metadata, timestamp};
use super::source::is_conversation_kind;
use super::template::Template;
use crate::domain::source::LibraryText;
use crate::{Error, Result};

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
            super::source::conversation::corrections::validate_user_text(kind, &text, "user")?;
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

/// One cited data item. `time` stays null for undated template suggestions.
#[derive(Debug, Serialize)]
pub struct Item {
    /// Stable object id, with a body anchor for an entry.
    pub id: String,
    /// Library-relative path (with optional anchor), or an Orbit task URI.
    pub link: String,
    /// Verbatim ordinary entry or object title; handoffs render structured
    /// fields in order while retaining scope and scribe markers.
    pub text: String,
    /// Recorded timestamp; never manufactured from the current clock.
    pub time: Option<String>,
    /// The capture kind for a quoted entry, else null.
    pub entry_kind: Option<String>,
    /// Task lifecycle state or typed run failure, else null.
    pub status: Option<String>,
    /// Template name for an onboarding suggestion, else null.
    pub template: Option<String>,
    /// Orbit workspace selector for a task read from an extra workspace
    /// (`brief.task_workspaces`); absent for the office workspace, so
    /// existing briefs serialize unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Additive correction marker, present only for corrected entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected: Option<bool>,
    /// Explicit user attribution on a captured entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Structured handoff data, absent on ordinary and historical entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_handoff: Option<handoff::Structured>,
    /// Historical unstructured handoff; its original text is retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy: Option<bool>,
    /// Thread, scope and state of a handoff in the list view
    /// (`view: list`); absent from every other output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<listing::Listing>,
}

/// A section in display order; optional sections are absent from the array.
#[derive(Debug, Serialize)]
pub struct Section {
    /// Stable section token documented by the response schema.
    pub kind: &'static str,
    /// Cited records, possibly empty.
    pub items: Vec<Item>,
    /// Matching items before the last-session excerpt limit.
    pub total: usize,
    /// Whether an explicit excerpt limit omitted records.
    pub truncated: bool,
    /// The page's first index in the list view's paged `history`; absent
    /// from every other section.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Section {
    fn new(kind: &'static str, items: Vec<Item>) -> Self {
        Self {
            kind,
            total: items.len(),
            items,
            truncated: false,
            offset: None,
        }
    }
}

/// A complete brief; section order is part of the tool contract.
#[derive(Debug, Serialize)]
pub struct Brief {
    /// Handoff, Global, Projects, Other sessions, Since, Waiting, Agents,
    /// Questions, Last session, Digest, Onboarding.
    pub sections: Vec<Section>,
}

/// The current session's thread identity, when the caller knows it. Both
/// fields are optional: a brief without a project keeps the legacy
/// single-conversation behaviour exactly.
#[derive(Clone, Debug, Default)]
pub struct SessionContext {
    /// Current project (a repository name or a working-directory path; it is
    /// normalized to the repository name before comparison).
    pub project: Option<String>,
    /// Current orchestrator (for example `claude` or `codex`), used to lead
    /// with the current orchestrator's thread inside the current project.
    /// It is the provider name conversation sources record, not a crew name.
    pub orchestrator: Option<String>,
    /// The current instant (RFC 3339) from the Clock port; a global handoff
    /// whose `until` is at or before it has expired. `None` never expires.
    pub now: Option<String>,
    /// Manual project choice (`brief.handoff: manual`, or `auto` with no
    /// single project): no project handoff and no `other_sessions`; the
    /// `projects` section lists the projects that have a thread instead.
    pub manual: bool,
}

/// An explicit `scope: global` declaration on a handoff entry
/// (specs/sessions.md#scopes). Set by the writer in `office.capture`, never
/// inferred: the stored marker is refused on a handoff without the scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalScope {
    /// Optional RFC 3339 expiry; the handoff leaves briefs once it passes.
    pub until: Option<String>,
}

impl GlobalScope {
    /// The marker leading the stored entry text: `(global)` or
    /// `(global until <RFC 3339>)`.
    #[must_use]
    pub fn marker(&self) -> String {
        match &self.until {
            Some(until) => format!("(global until {until})"),
            None => "(global)".into(),
        }
    }
}

/// Whether stored handoff text starts with a global marker, well-formed or
/// not. Capture refuses such text on a handoff that did not declare the
/// scope, so the marker cannot be forged by ordinary entry text.
#[must_use]
pub fn looks_global(text: &str) -> bool {
    text.starts_with("(global)") || text.starts_with("(global until ")
}

/// The scope a stored handoff text declares: `Ok(None)` for a project
/// handoff, the scope for `(global) …` or `(global until <time>) …`, and an
/// error for a malformed marker.
pub fn stored_scope(text: &str) -> Result<Option<GlobalScope>> {
    if !looks_global(text) {
        return Ok(None);
    }
    if let Some(rest) = text.strip_prefix("(global)") {
        if rest.starts_with(' ') && !rest.trim().is_empty() {
            return Ok(Some(GlobalScope::default()));
        }
        return Err(invalid("global handoff marker"));
    }
    let rest = text
        .strip_prefix("(global until ")
        .ok_or_else(|| invalid("global handoff marker"))?;
    let (until, tail) = rest
        .split_once(')')
        .ok_or_else(|| invalid("global handoff marker"))?;
    if timestamp(until).is_none() || !tail.starts_with(' ') || tail.trim().is_empty() {
        return Err(invalid("global handoff until"));
    }
    Ok(Some(GlobalScope {
        until: Some(until.into()),
    }))
}

/// `other_sessions`-style cap for the `global` section: at most this many
/// open global handoffs are listed, newest first.
pub const GLOBAL_HANDOFFS_LIMIT: usize = 5;
/// Cap for the `projects` section (projects with a handoff thread).
pub const PROJECTS_LIMIT: usize = 20;

/// `other_sessions` window: only handoffs at most this many seconds older
/// than the scoped `since` reference are listed.
pub const OTHER_SESSIONS_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;
/// `other_sessions` cap: at most this many other threads are listed.
pub const OTHER_SESSIONS_LIMIT: usize = 5;

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

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(field))
}

// Compare instants, not differently offset timestamp strings. Preserve arbitrary
// fractional precision as well as the original spelling returned to the caller.
fn time_key(at: &str) -> Result<(i64, String)> {
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

struct Conversation {
    id: String,
    path: String,
    title: String,
    time: String,
    log: Vec<Entry>,
    project: Option<String>,
    orchestrator: Option<String>,
    session_id: Option<String>,
    /// `sensitivity: private`: the list view withholds its entry text.
    private: bool,
    /// Lines of original `handoff` entries that declared `scope: global`.
    /// Read from the original log, so a correction cannot change a scope.
    globals: BTreeMap<usize, GlobalScope>,
}

/// An optional single-line frontmatter field: absent or null is `None`, a
/// nonempty string is kept, anything else refuses the read.
fn optional_text(fields: &Value, field: &str) -> Result<Option<String>> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(invalid(field)),
    }
}

/// Normalize a project value to the repository name: a trailing
/// `.claude/worktrees/<name>` names its repository, otherwise the last path
/// component; a bare name is unchanged.
fn project_key(project: Option<&str>) -> Option<String> {
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
fn cited_assignment(text: &str) -> Option<String> {
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
fn cites(text: &str, anchor: &str) -> bool {
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

impl Conversation {
    /// The newest handoff in this log that belongs to its project thread.
    fn project_handoff(&self) -> Option<&Entry> {
        self.log
            .iter()
            .rev()
            .find(|e| e.kind == "handoff" && !self.globals.contains_key(&e.line))
    }
    /// Whether `entry` (in `self`) cites the global handoff `global` by
    /// anchor: its full id from anywhere, or its local anchor in its own log.
    fn entry_cites(&self, entry: &Entry, global: &GlobalHandoff<'_>) -> bool {
        self.entry_cites_line(entry, global.conversation, global.entry)
    }
    /// Whether `entry` (in `self`) cites `target` (in `owner`) by anchor.
    fn entry_cites_line(&self, entry: &Entry, owner: &Conversation, target: &Entry) -> bool {
        cites(&entry.text, &format!("{}#L{}", owner.id, target.line))
            || (self.id == owner.id && cites(&entry.text, &format!("#L{}", target.line)))
    }
    fn item(&self, entry: &Entry) -> Item {
        // conversations() validates the effective entries before projection.
        let structured_handoff = (entry.kind == "handoff")
            .then(|| handoff::decode(&entry.text).ok().flatten())
            .flatten();
        let legacy = (entry.kind == "handoff" && structured_handoff.is_none()).then_some(true);
        let text = structured_handoff
            .as_ref()
            .map_or_else(|| entry.text.clone(), |h| handoff::render(&entry.text, h));
        Item {
            id: format!("{}#L{}", self.id, entry.line),
            link: format!("{}#L{}", self.path, entry.line),
            text,
            time: Some(entry.time.clone()),
            entry_kind: Some(entry.kind.clone()),
            status: None,
            template: None,
            workspace: None,
            corrected: entry.corrected.then_some(true),
            by: entry.by.clone(),
            structured_handoff,
            legacy,
            listing: None,
        }
    }
    fn reference(&self) -> Item {
        Item {
            id: self.id.clone(),
            link: self.path.clone(),
            text: self.title.clone(),
            time: Some(self.time.clone()),
            entry_kind: None,
            status: None,
            template: None,
            workspace: None,
            corrected: None,
            by: None,
            structured_handoff: None,
            legacy: None,
            listing: None,
        }
    }
}

fn conversations(files: &[LibraryText]) -> Result<Vec<Conversation>> {
    let mut result = Vec::new();
    for file in files {
        let raw = markdown(file)?;
        let fields = Value::Object(metadata(raw)?);
        if text(&fields, "kind")? != "conversation" {
            continue;
        }
        let (_, body) = frontmatter::split(raw)?;
        let log = super::source::conversation::corrections::effective_entries(body)?;
        let mut time = text(&fields, "captured_at")?.to_string();
        let mut key = time_key(&time)?;
        let mut globals = BTreeMap::new();
        // Activity includes correction/removal audit records even when the
        // effective evidence is empty or retains an older timestamp.
        for entry in entries(body)? {
            let next = time_key(&entry.time)?;
            if next >= key {
                time.clone_from(&entry.time);
                key = next;
            }
            if entry.kind == "handoff"
                && let Some(scope) = stored_scope(&entry.text)?
            {
                globals.insert(entry.line, scope);
            }
        }
        result.push(Conversation {
            id: text(&fields, "id")?.into(),
            title: text(&fields, "title")?.into(),
            path: file.path.clone(),
            time,
            log,
            project: optional_text(&fields, "project")?,
            orchestrator: optional_text(&fields, "orchestrator")?,
            session_id: optional_text(&fields, "session_id")?,
            private: optional_text(&fields, "sensitivity")?.as_deref() == Some("private"),
            globals,
        });
    }
    // Keys were validated above; sorting never maps an unknown timestamp to zero.
    let mut keyed = result
        .into_iter()
        .map(|c| Ok((time_key(&c.time)?, c)))
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| y.id.cmp(&x.id)));
    Ok(keyed.into_iter().map(|(_, c)| c).collect())
}

/// Validate a complete Orbit projection, including pagination metadata.
pub fn tasks(value: &Value) -> Result<&[Value]> {
    let rows = value
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("orbit.task.list.tasks"))?;
    match value.get("truncated").and_then(Value::as_bool) {
        Some(false) => {}
        Some(true) => return Err(Error::Library {
            code: "task_list_truncated",
            message:
                "orbit.task.list is truncated; session brief requires the complete task inventory"
                    .into(),
        }),
        None => return Err(invalid("orbit.task.list.truncated")),
    }
    if value.get("total").and_then(Value::as_u64) != u64::try_from(rows.len()).ok() {
        return Err(invalid("orbit.task.list.total"));
    }
    let mut ids = BTreeSet::new();
    for row in rows {
        if !ids.insert(text(row, "id")?) {
            return Err(invalid("unique task ids"));
        }
        tags(row)?;
    }
    Ok(rows)
}

fn tags(row: &Value) -> Result<Vec<&str>> {
    row.get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("task.tags"))?
        .iter()
        .map(|t| t.as_str().ok_or_else(|| invalid("task.tags")))
        .collect()
}

/// Runs whose typed delivery evidence is needed to distinguish failure from a
/// blocked task or a later metadata edit. No failure is inferred from prose.
pub fn agent_runs(rows: &[Value]) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for row in rows {
        let tags = tags(row)?;
        if tags.contains(&"owner:agent")
            && !tags.contains(&"proposal")
            && task_active(text(row, "status")?)?
        {
            match row.get("job_run_id") {
                Some(Value::String(id)) if !id.is_empty() => {
                    result.push((text(row, "id")?.into(), id.clone()))
                }
                None | Some(Value::Null) => {}
                Some(_) => return Err(invalid("task.job_run_id")),
            }
        }
    }
    Ok(result)
}

/// Whether an Orbit task status is open (not `done`, `rejected` or
/// `archived`); an unknown status is not open.
#[must_use]
pub fn task_open(status: &str) -> bool {
    task_active(status).unwrap_or(false)
}

fn task_active(status: &str) -> Result<bool> {
    match status {
        "proposed" | "backlog" | "in-progress" | "review" | "blocked" | "someday" => Ok(true),
        "done" | "rejected" | "archived" => Ok(false),
        _ => Err(invalid("task.status")),
    }
}

fn task_item(row: &Value, at: &str, status: &str, workspace: Option<&str>) -> Result<Item> {
    let id = text(row, "id")?;
    Ok(Item {
        id: id.into(),
        link: format!("orbit://task/{id}"),
        text: text(row, "title")?.into(),
        time: Some(at.into()),
        entry_kind: None,
        status: Some(status.into()),
        template: None,
        workspace: workspace.map(str::to_string),
        corrected: None,
        by: None,
        structured_handoff: None,
        legacy: None,
        listing: None,
    })
}

/// One workspace's validated task inventory. `workspace: None` is the office
/// workspace; `Some(selector)` is an extra workspace from
/// `brief.task_workspaces`, read through `orbit.task.list` with that selector.
pub struct TaskInventory<'a> {
    /// Extra workspace selector, or `None` for the office workspace.
    pub workspace: Option<&'a str>,
    /// Validated rows from that workspace's `orbit.task.list`.
    pub rows: &'a [Value],
}

fn task_sections(
    inventories: &[TaskInventory<'_>],
    since: Option<&str>,
    deliveries: &BTreeMap<String, Value>,
) -> Result<(Section, Section)> {
    let cutoff = since.map(time_key).transpose()?;
    let mut waiting = Vec::new();
    let mut working = Vec::new();
    let mut seen = BTreeSet::new();
    for inventory in inventories {
        let external = inventory.workspace.is_some();
        for row in inventory.rows {
            if !seen.insert(text(row, "id")?.to_string()) {
                continue;
            }
            let item = classify_task(
                row,
                inventory.workspace,
                external,
                &cutoff,
                deliveries,
                &mut waiting,
                &mut working,
            );
            // An extra workspace's malformed row must not refuse the whole
            // brief; the handler reports such a workspace as unavailable
            // before assembly, so a residual error here is skipped only for
            // external rows. Office-workspace errors stay all-or-nothing.
            match item {
                Ok(()) => {}
                Err(error) if external => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
    waiting.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.id.cmp(&y.id)));
    working.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.id.cmp(&y.id)));
    Ok((
        Section::new(
            "waiting_on_you",
            waiting.into_iter().map(|(_, i)| i).collect(),
        ),
        Section::new(
            "agents_working_on",
            working.into_iter().map(|(_, i)| i).collect(),
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
fn classify_task(
    row: &Value,
    workspace: Option<&str>,
    external: bool,
    cutoff: &Option<(i64, String)>,
    deliveries: &BTreeMap<String, Value>,
    waiting: &mut Vec<((i64, String), Item)>,
    working: &mut Vec<((i64, String), Item)>,
) -> Result<()> {
    {
        let tags = tags(row)?;
        let agent = tags.contains(&"owner:agent");
        let you = tags.contains(&"owner:you");
        let proposal = tags.contains(&"proposal");
        // Office workspace: only office-tagged work, exactly as before.
        // Extra workspaces do not use office ownership tags, so there the
        // documented rule is broader: any `review` task waits on the user,
        // and any non-proposal in-progress/finished/failed task is working.
        if !external && !agent && !you && !proposal {
            return Ok(());
        }
        let status = text(row, "status")?;
        let active = task_active(status)?;
        let created = text(row, "created_at")?;
        time_key(created)?;
        let waits = (proposal && status == "proposed")
            || (!proposal
                && ((agent && status == "review")
                    || (you && active)
                    || (external && status == "review")));
        if waits {
            waiting.push((
                time_key(created)?,
                task_item(row, created, status, workspace)?,
            ));
        }
        if proposal || (!external && !agent) {
            return Ok(());
        }
        let mut at = None;
        let mut working_status = status;
        if status == "in-progress" {
            at = Some(text(row, "updated_at")?.to_string());
        }
        if status == "done" {
            let history = row
                .get("history")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("task.history"))?;
            // Completion is the latest transition to done, never updated_at.
            let mut finished = history
                .iter()
                .filter(|e| e.get("to_status").and_then(Value::as_str) == Some("done"))
                .map(|e| {
                    let at = text(e, "at")?;
                    Ok((time_key(at)?, at.to_string()))
                })
                .collect::<Result<Vec<_>>>()?;
            finished.sort();
            let (key, stamp) = finished
                .pop()
                .ok_or_else(|| invalid("task completion history"))?;
            if cutoff.as_ref().is_none_or(|since| key > *since) {
                at = Some(stamp);
            }
        }
        if let Some(delivery) = deliveries.get(text(row, "id")?) {
            let state = text(delivery, "run_state")?;
            if active && state == "failed" {
                // A failed run is no longer working, even if task promotion has
                // not yet changed its in-progress lifecycle state.
                at = None;
                let stamp = text(delivery, "run_finished_at")?;
                let key = time_key(stamp)?;
                if cutoff.as_ref().is_none_or(|since| key > *since) {
                    at = Some(stamp.into());
                    working_status = "failed";
                }
            }
        }
        if let Some(at) = at {
            working.push((
                time_key(&at)?,
                task_item(row, &at, working_status, workspace)?,
            ));
        }
        Ok(())
    }
}

/// Runs needing delivery evidence in an extra workspace, where office
/// ownership tags do not apply: any active non-proposal task with a run.
pub fn agent_runs_external(rows: &[Value]) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for row in rows {
        let tags = tags(row)?;
        if tags.contains(&"proposal") || !task_active(text(row, "status")?)? {
            continue;
        }
        match row.get("job_run_id") {
            Some(Value::String(id)) if !id.is_empty() => {
                result.push((text(row, "id")?.into(), id.clone()))
            }
            None | Some(Value::Null) => {}
            Some(_) => return Err(invalid("task.job_run_id")),
        }
    }
    Ok(result)
}

/// Assemble an all-or-nothing brief from complete, read-only port snapshots.
/// Recent questions cover the ten most recently appended conversation sources;
/// the last-session excerpt contains at most five decisions/directions.
pub fn assemble(
    sources: &[LibraryText],
    rows: &[Value],
    artifacts: &[LibraryText],
    pages: &[LibraryText],
    templates: &[LibraryText],
    deliveries: &BTreeMap<String, Value>,
) -> Result<Brief> {
    assemble_with_workspaces(
        sources,
        &[TaskInventory {
            workspace: None,
            rows,
        }],
        &[],
        artifacts,
        pages,
        templates,
        deliveries,
    )
}

struct ThreadHandoff<'a> {
    key: String,
    conversation: &'a Conversation,
    entry: &'a Entry,
    at: (i64, String),
}

/// The thread a project handoff belongs to: `task:<id>` for the first
/// assignment id it cites, else its conversation's normalized project plus
/// orchestrator (separated by a NUL, which no frontmatter value contains).
fn thread_key(conversation: &Conversation, entry: &Entry) -> String {
    match cited_assignment(&entry.text) {
        Some(id) => format!("task:{id}"),
        None => format!(
            "project:{}\u{0}orchestrator:{}",
            project_key(conversation.project.as_deref()).unwrap_or_default(),
            conversation.orchestrator.as_deref().unwrap_or("")
        ),
    }
}

/// One thread per assignment id a handoff cites, else per normalized
/// project plus orchestrator; each thread keeps only its newest handoff.
fn thread_handoffs<'a>(conversations: &'a [Conversation]) -> Result<Vec<ThreadHandoff<'a>>> {
    let mut by_key: BTreeMap<String, ThreadHandoff<'a>> = BTreeMap::new();
    for conversation in conversations {
        let Some(entry) = conversation.project_handoff() else {
            continue;
        };
        let key = thread_key(conversation, entry);
        let candidate = ThreadHandoff {
            key: key.clone(),
            conversation,
            entry,
            at: time_key(&entry.time)?,
        };
        let keep_existing = by_key.get(&key).is_some_and(|existing| {
            (existing.at.clone(), &existing.conversation.id)
                >= (candidate.at.clone(), &candidate.conversation.id)
        });
        if !keep_existing {
            by_key.insert(key, candidate);
        }
    }
    Ok(by_key.into_values().collect())
}

/// An open global handoff: not expired and not closed by a `resolved` entry.
struct GlobalHandoff<'a> {
    conversation: &'a Conversation,
    entry: &'a Entry,
    at: (i64, String),
}

/// Every open global handoff, newest first. A global is closed by a
/// `resolved` entry recorded at or after it that cites it (its full
/// `<source-id>#L<n>` from any conversation, or its local `#L<n>` later in
/// its own log); it expires when its `until` is at or before `now`.
fn open_globals<'a>(
    conversations: &'a [Conversation],
    now: Option<&str>,
) -> Result<Vec<GlobalHandoff<'a>>> {
    let now = now.map(time_key).transpose()?;
    let mut globals = Vec::new();
    for conversation in conversations {
        for entry in &conversation.log {
            if entry.kind != "handoff" {
                continue;
            }
            let Some(scope) = conversation.globals.get(&entry.line) else {
                continue;
            };
            if let (Some(until), Some(now)) = (scope.until.as_deref(), now.as_ref())
                && time_key(until)? <= *now
            {
                continue;
            }
            globals.push(GlobalHandoff {
                conversation,
                entry,
                at: time_key(&entry.time)?,
            });
        }
    }
    let mut open = Vec::new();
    for global in globals {
        if closing(conversations, global.conversation, global.entry)?.is_none() {
            open.push(global);
        }
    }
    open.sort_by(|a, b| {
        (b.at.clone(), &b.conversation.id, b.entry.line).cmp(&(
            a.at.clone(),
            &a.conversation.id,
            a.entry.line,
        ))
    });
    Ok(open)
}

/// The earliest `resolved` entry that closes `target` (in `owner`): one
/// recorded at or after it that cites it by its full `<source-id>#L<n>`
/// from any conversation, or by its local `#L<n>` later in its own log.
fn closing<'a>(
    conversations: &'a [Conversation],
    owner: &Conversation,
    target: &Entry,
) -> Result<Option<(&'a Conversation, &'a Entry)>> {
    let at = time_key(&target.time)?;
    let mut found: Option<((i64, String), &'a Conversation, &'a Entry)> = None;
    for conversation in conversations {
        for entry in conversation.log.iter().filter(|e| e.kind == "resolved") {
            let entry_at = time_key(&entry.time)?;
            let later = if conversation.id == owner.id {
                entry.line > target.line
            } else {
                entry_at >= at
            };
            if later
                && conversation.entry_cites_line(entry, owner, target)
                && found
                    .as_ref()
                    .is_none_or(|(best, best_conversation, best_entry)| {
                        (&entry_at, &conversation.id, entry.line)
                            < (best, &best_conversation.id, best_entry.line)
                    })
            {
                found = Some((entry_at, conversation, entry));
            }
        }
    }
    Ok(found.map(|(_, conversation, entry)| (conversation, entry)))
}

/// The project the caller asked for, normalized; none in manual mode.
fn scoped_project(context: &SessionContext) -> Option<String> {
    if context.manual {
        return None;
    }
    context
        .project
        .as_deref()
        .and_then(|project| project_key(Some(project)))
}

/// The lead handoff: a scope's latest SCR, else (the 30-day window) its
/// legacy thread handoff.
enum Lead<'a> {
    Scr(&'a scr::ScrHandoff),
    Legacy(&'a Conversation, &'a Entry),
}

impl Lead<'_> {
    fn item(&self) -> Item {
        match self {
            Self::Scr(scr) => scr.item(),
            Self::Legacy(conversation, entry) => conversation.item(entry),
        }
    }
    /// Whether the lead cites the legacy global `global` by anchor.
    fn cites_global(&self, global: &GlobalHandoff<'_>) -> bool {
        match self {
            Self::Scr(scr) => scr.cites_anchor(&format!(
                "{}#L{}",
                global.conversation.id, global.entry.line
            )),
            Self::Legacy(conversation, entry) => conversation.entry_cites(entry, global),
        }
    }
}

/// The handoff sections, in contract order: `handoff` (the project's latest
/// SCR; a project with no SCR keeps its legacy thread handoff), `global`
/// (the latest global SCR first, then open legacy global handoffs marked
/// legacy, newest first, capped at [`GLOBAL_HANDOFFS_LIMIT`] in all; a
/// legacy global the lead cites is always listed, exactly once),
/// `projects` (manual mode only) and, when `include_others` and a project
/// is given, `other_sessions` (the project's other SCRs within seven days
/// of its newest, or its legacy other threads when it has no SCR).
fn handoff_sections(
    conversations: &[Conversation],
    scrs: &[scr::ScrHandoff],
    context: &SessionContext,
    include_others: bool,
) -> Result<Vec<Section>> {
    let globals = open_globals(conversations, context.now.as_deref())?;
    let global_scr = scr::latest_global(scrs);
    let mut lead: Option<Lead<'_>> = None;
    let mut others_section = None;
    if let Some(wanted) = scoped_project(context) {
        let project_scrs = scr::in_project(scrs, &wanted);
        if let Some((newest, rest)) = project_scrs.split_first() {
            lead = Some(Lead::Scr(newest));
            if include_others {
                let others: Vec<&scr::ScrHandoff> = rest
                    .iter()
                    .copied()
                    .filter(|other| {
                        other.at.0 >= newest.at.0 - OTHER_SESSIONS_WINDOW_SECONDS
                            && other.at.0 <= newest.at.0
                    })
                    .collect();
                if !others.is_empty() {
                    let total = others.len();
                    let mut section = Section::new(
                        "other_sessions",
                        others
                            .iter()
                            .take(OTHER_SESSIONS_LIMIT)
                            .map(|other| other.item())
                            .collect(),
                    );
                    section.total = total;
                    section.truncated = total > section.items.len();
                    others_section = Some(section);
                }
            }
        } else {
            let threads = thread_handoffs(conversations)?;
            let in_project: Vec<&ThreadHandoff<'_>> = threads
                .iter()
                .filter(|thread| {
                    project_key(thread.conversation.project.as_deref()).as_ref() == Some(&wanted)
                })
                .collect();
            let own: Vec<&ThreadHandoff<'_>> = match context.orchestrator.as_deref() {
                Some(orchestrator) => in_project
                    .iter()
                    .copied()
                    .filter(|thread| {
                        thread.conversation.orchestrator.as_deref() == Some(orchestrator)
                    })
                    .collect(),
                None => Vec::new(),
            };
            let pool: &[&ThreadHandoff<'_>] = if own.is_empty() { &in_project } else { &own };
            let lead_thread = pool.iter().copied().max_by(|a, b| {
                (a.at.clone(), &a.conversation.id).cmp(&(b.at.clone(), &b.conversation.id))
            });
            lead = lead_thread.map(|thread| Lead::Legacy(thread.conversation, thread.entry));
            // The window is measured from the newest conversation overall, so a
            // newer thread in another project is never excluded as "future".
            if include_others && let Some(reference) = conversations.first() {
                let (reference_seconds, _) = time_key(&reference.time)?;
                let mut others: Vec<&ThreadHandoff<'_>> = threads
                    .iter()
                    .filter(|thread| {
                        Some(thread.key.as_str()) != lead_thread.map(|lead| lead.key.as_str())
                    })
                    .filter(|thread| {
                        thread.at.0 >= reference_seconds - OTHER_SESSIONS_WINDOW_SECONDS
                            && thread.at.0 <= reference_seconds
                    })
                    .collect();
                others.sort_by(|a, b| {
                    (b.at.clone(), &b.conversation.id).cmp(&(a.at.clone(), &a.conversation.id))
                });
                if !others.is_empty() {
                    let total = others.len();
                    let mut section = Section::new(
                        "other_sessions",
                        others
                            .iter()
                            .take(OTHER_SESSIONS_LIMIT)
                            .map(|thread| thread.conversation.item(thread.entry))
                            .collect(),
                    );
                    section.total = total;
                    section.truncated = total > section.items.len();
                    others_section = Some(section);
                }
            }
        }
    } else if !context.manual {
        // No project named: the newest project SCR overall, else the
        // legacy newest conversation's handoff.
        if let Some(newest) = scrs.iter().find(|scr| scr.project().is_some()) {
            lead = Some(Lead::Scr(newest));
        } else if let Some(c) = conversations.first()
            && let Some(entry) = c.project_handoff()
        {
            lead = Some(Lead::Legacy(c, entry));
        }
    }
    let mut sections = Vec::new();
    if let Some(lead_item) = &lead {
        sections.push(Section::new("handoff", vec![lead_item.item()]));
    }
    let global_total = usize::from(global_scr.is_some()) + globals.len();
    if global_total > 0 {
        let mut items: Vec<Item> = global_scr.map(|scr| scr.item()).into_iter().collect();
        for (index, global) in globals.iter().enumerate() {
            let within_cap = index + usize::from(global_scr.is_some()) < GLOBAL_HANDOFFS_LIMIT;
            if !within_cap && !lead.as_ref().is_some_and(|lead| lead.cites_global(global)) {
                continue;
            }
            let mut item = global.conversation.item(global.entry);
            // After the global SCR, window legacy globals are marked; their
            // rendered text (structured included) is unchanged.
            if global_scr.is_some() {
                item.legacy = Some(true);
            }
            items.push(item);
        }
        let mut section = Section::new("global", items);
        section.total = global_total;
        section.truncated = global_total > section.items.len();
        sections.push(section);
    }
    if context.manual {
        let projects = project_threads(conversations, scrs)?;
        if !projects.is_empty() {
            let total = projects.len();
            let mut section = Section::new(
                "projects",
                projects.into_iter().take(PROJECTS_LIMIT).collect(),
            );
            section.total = total;
            section.truncated = total > section.items.len();
            sections.push(section);
        }
    }
    sections.extend(others_section);
    Ok(sections)
}

/// One item per project with a handoff, newest first: the project's latest
/// SCR, or (the window) its newest legacy thread handoff when it has no
/// SCR. `id` and `text` name the project; `link` and `time` are its
/// handoff's (the SCR path and its `updated`). Conversations that record
/// no project are not listed.
fn project_threads(conversations: &[Conversation], scrs: &[scr::ScrHandoff]) -> Result<Vec<Item>> {
    fn project_item(project: &str, link: String, time: String) -> Item {
        Item {
            id: project.to_string(),
            link,
            text: project.to_string(),
            time: Some(time),
            entry_kind: None,
            status: None,
            template: None,
            workspace: None,
            corrected: None,
            by: None,
            structured_handoff: None,
            legacy: None,
            listing: None,
        }
    }
    let mut rows: Vec<((i64, String), String, Item)> = Vec::new();
    let scr_projects = scr::latest_per_project(scrs);
    for (project, scr) in &scr_projects {
        rows.push((
            scr.at.clone(),
            project.clone(),
            project_item(project, scr.scr.path.clone(), scr.scr.front.updated.clone()),
        ));
    }
    let mut newest: BTreeMap<String, &ThreadHandoff<'_>> = BTreeMap::new();
    let threads = thread_handoffs(conversations)?;
    for thread in &threads {
        let Some(project) = project_key(thread.conversation.project.as_deref()) else {
            continue;
        };
        if scr_projects.contains_key(&project) {
            continue;
        }
        let replace = newest.get(&project).is_none_or(|existing| {
            (existing.at.clone(), &existing.conversation.id)
                < (thread.at.clone(), &thread.conversation.id)
        });
        if replace {
            newest.insert(project, thread);
        }
    }
    for (project, thread) in newest {
        rows.push((
            thread.at.clone(),
            project.clone(),
            project_item(
                &project,
                format!("{}#L{}", thread.conversation.path, thread.entry.line),
                thread.entry.time.clone(),
            ),
        ));
    }
    rows.sort_by(|(a_at, x, _), (b_at, y, _)| (b_at.clone(), y).cmp(&(a_at.clone(), x)));
    Ok(rows.into_iter().map(|(_, _, item)| item).collect())
}

/// The project of an assignment: the project of the newest SCR whose Next
/// cites `task`, else (the window) the normalized project of the newest
/// conversation that recorded a project handoff citing it (the thread key
/// of [`thread_handoffs`]). Orbit's task record carries no workspace path,
/// so the Library is where the workspace is known.
pub fn assignment_project(
    sources: &[LibraryText],
    scr_files: &[LibraryText],
    task: &str,
) -> Result<Option<String>> {
    for scr in &scr::parse(scr_files)? {
        if scr.next_cites(task)
            && let Some(project) = scr.project()
        {
            return Ok(Some(project));
        }
    }
    let conversations = conversations(sources)?;
    for conversation in &conversations {
        if conversation.project.is_none() {
            continue;
        }
        let cites_task = conversation.log.iter().any(|entry| {
            entry.kind == "handoff"
                && !conversation.globals.contains_key(&entry.line)
                && cited_assignment(&entry.text).as_deref() == Some(task)
        });
        if cites_task {
            return Ok(project_key(conversation.project.as_deref()));
        }
    }
    Ok(None)
}

/// The handoff view behind `/handoff`: with a project, its latest SCR
/// (else its legacy thread handoff) then the global section; without one,
/// the global section and the projects that have a handoff. No task or
/// digest reads.
pub fn handoffs(
    sources: &[LibraryText],
    scr_files: &[LibraryText],
    context: &SessionContext,
) -> Result<Brief> {
    let conversations = conversations(sources)?;
    let scrs = scr::parse(scr_files)?;
    let mut context = context.clone();
    context.manual = context.manual || scoped_project(&context).is_none();
    Ok(Brief {
        sections: handoff_sections(&conversations, &scrs, &context, false)?,
    })
}

/// Assemble a brief merging extra workspaces. The office workspace keeps
/// the all-or-nothing rule; each selector in `unavailable` becomes one
/// `workspace_unavailable` item instead of refusing the brief.
#[allow(clippy::too_many_arguments)]
pub fn assemble_with_workspaces(
    sources: &[LibraryText],
    inventories: &[TaskInventory<'_>],
    unavailable: &[String],
    artifacts: &[LibraryText],
    pages: &[LibraryText],
    templates: &[LibraryText],
    deliveries: &BTreeMap<String, Value>,
) -> Result<Brief> {
    assemble_for_session(
        sources,
        &[],
        inventories,
        unavailable,
        artifacts,
        pages,
        templates,
        deliveries,
        &SessionContext::default(),
    )
}

/// Assemble a brief for the current session. With a project, the `handoff`
/// section is that project's latest SCR (a project with no SCR keeps its
/// legacy thread handoff: its project, preferring its orchestrator),
/// `global` lists the latest global SCR then the open legacy globals,
/// `other_sessions` lists the project's other recent SCRs (or its legacy
/// other threads), and `since` is measured from the newest conversation
/// in that project. Without a project the newest project SCR leads, else
/// the legacy single-conversation behaviour is kept, plus `global`; in
/// manual mode there is no `handoff` or `other_sessions` and `projects`
/// lists the projects with a handoff.
#[allow(clippy::too_many_arguments)]
pub fn assemble_for_session(
    sources: &[LibraryText],
    scr_files: &[LibraryText],
    inventories: &[TaskInventory<'_>],
    unavailable: &[String],
    artifacts: &[LibraryText],
    pages: &[LibraryText],
    templates: &[LibraryText],
    deliveries: &BTreeMap<String, Value>,
    context: &SessionContext,
) -> Result<Brief> {
    let conversations = conversations(sources)?;
    let scrs = scr::parse(scr_files)?;
    let scoped_project = scoped_project(context);
    let last = match &scoped_project {
        Some(wanted) => conversations
            .iter()
            .find(|c| project_key(c.project.as_deref()).as_ref() == Some(wanted)),
        None => conversations.first(),
    };
    let mut sections = handoff_sections(&conversations, &scrs, context, true)?;
    sections.push(Section::new(
        "since",
        last.map(Conversation::reference).into_iter().collect(),
    ));
    let (waiting, working) = task_sections(inventories, last.map(|c| c.time.as_str()), deliveries)?;
    sections.push(waiting);
    sections.push(working);
    if !unavailable.is_empty() {
        let mut selectors = unavailable.to_vec();
        selectors.sort();
        selectors.dedup();
        sections.push(Section::new(
            "workspace_unavailable",
            selectors
                .into_iter()
                .map(|selector| Item {
                    id: selector.clone(),
                    link: format!("orbit://workspace/{selector}"),
                    text: selector.clone(),
                    time: None,
                    entry_kind: None,
                    status: Some("workspace_unavailable".into()),
                    template: None,
                    workspace: Some(selector),
                    corrected: None,
                    by: None,
                    structured_handoff: None,
                    legacy: None,
                    listing: None,
                })
                .collect(),
        ));
    }
    let mut questions = Vec::new();
    for c in conversations.iter().take(10) {
        let mut open = BTreeMap::new();
        for entry in &c.log {
            if entry.kind == "open" {
                open.insert(entry.line, entry);
            }
            if entry.kind == "resolved" {
                // A resolution starts with the exact local #L<n> anchor;
                // mentions elsewhere and resolutions before an open do not close it.
                let token = entry.text.split_whitespace().next().unwrap_or("");
                if let Some(line) = token
                    .strip_prefix("#L")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    open.remove(&line);
                }
            }
        }
        questions.extend(open.values().map(|e| c.item(e)));
    }
    sections.push(Section::new("open_questions", questions));
    let mut recent = Vec::new();
    let mut total = 0;
    if let Some(c) = last {
        recent.push(c.reference());
        let decisions: Vec<_> = c
            .log
            .iter()
            .filter(|e| e.kind == "decision" || e.kind == "direction")
            .collect();
        total = decisions.len() + 1;
        recent.extend(
            decisions
                .iter()
                .skip(decisions.len().saturating_sub(5))
                .map(|e| c.item(e)),
        );
    }
    let mut session = Section::new("last_session", recent);
    session.truncated = total > session.items.len();
    session.total = total;
    sections.push(session);
    let mut digests = Vec::new();
    for file in artifacts {
        let artifact = Artifact::from_document(&file.path, markdown(file)?)?;
        if artifact.series == "digest-daily" {
            digests.push((
                time_key(&artifact.approved.at)?,
                artifact.id.clone(),
                file.path.clone(),
                artifact,
            ));
        }
    }
    digests.sort_by(|(a, x, _, _), (b, y, _, _)| a.cmp(b).then_with(|| x.cmp(y)));
    if let Some((_, _, path, digest)) = digests.pop() {
        sections.push(Section::new(
            "latest_digest",
            vec![Item {
                id: digest.id,
                link: path,
                text: digest.title,
                time: Some(digest.approved.at),
                entry_kind: None,
                status: None,
                template: None,
                workspace: None,
                corrected: None,
                by: None,
                structured_handoff: None,
                legacy: None,
                listing: None,
            }],
        ));
    }
    let present = pages
        .iter()
        .map(|file| Page::from_markdown(markdown(file)?))
        .collect::<Result<Vec<_>>>()?;
    let mut missing = Vec::new();
    for file in templates {
        let template = Template::from_markdown(&file.path, markdown(file)?)?;
        if template.one_per_library
            && !present
                .iter()
                .any(|page| page.template.as_deref() == Some(template.template.as_str()))
        {
            missing.push(Item {
                id: template.id.as_str().into(),
                link: file.path.clone(),
                text: template.title,
                time: None,
                entry_kind: None,
                status: None,
                template: Some(template.template),
                workspace: None,
                corrected: None,
                by: None,
                structured_handoff: None,
                legacy: None,
                listing: None,
            });
        }
    }
    missing.sort_by(|a, b| a.id.cmp(&b.id));
    if !missing.is_empty() {
        sections.push(Section::new("onboarding", missing));
    }
    Ok(Brief { sections })
}
