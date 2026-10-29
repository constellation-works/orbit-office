//! Deterministic session continuity. All inputs are recorded data; nothing here
//! interprets a quoted entry as an instruction or performs I/O.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::artifact::Artifact;
use super::knowledge::{Page, frontmatter};
use super::metrics::{metadata, timestamp};
use super::source::is_conversation_kind;
use super::template::Template;
use crate::ports::LibraryText;
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
        let (kind, text) = entry.split_once(": ").ok_or_else(|| invalid("log entry"))?;
        time_key(time)?;
        if !is_conversation_kind(kind) || text.trim().is_empty() {
            return Err(invalid("log entry kind or text"));
        }
        result.push(Entry {
            line: index + 1,
            time: time.into(),
            kind: kind.into(),
            text: text.into(),
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
    /// Verbatim entry text or the recorded object's title.
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
}

impl Section {
    fn new(kind: &'static str, items: Vec<Item>) -> Self {
        Self {
            kind,
            total: items.len(),
            items,
            truncated: false,
        }
    }
}

/// A complete brief; section order is part of the tool contract.
#[derive(Debug, Serialize)]
pub struct Brief {
    /// Handoff, Since, Waiting, Agents, Questions, Last session, Digest, Onboarding.
    pub sections: Vec<Section>,
}

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
}

impl Conversation {
    fn item(&self, entry: &Entry) -> Item {
        Item {
            id: format!("{}#L{}", self.id, entry.line),
            link: format!("{}#L{}", self.path, entry.line),
            text: entry.text.clone(),
            time: Some(entry.time.clone()),
            entry_kind: Some(entry.kind.clone()),
            status: None,
            template: None,
            workspace: None,
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
        let log = entries(body)?;
        let mut time = text(&fields, "captured_at")?.to_string();
        let mut key = time_key(&time)?;
        for entry in &log {
            let next = time_key(&entry.time)?;
            if next >= key {
                time.clone_from(&entry.time);
                key = next;
            }
        }
        result.push(Conversation {
            id: text(&fields, "id")?.into(),
            title: text(&fields, "title")?.into(),
            path: file.path.clone(),
            time,
            log,
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
    let conversations = conversations(sources)?;
    let last = conversations.first();
    let mut sections = Vec::new();
    if let Some(c) = last
        && let Some(entry) = c.log.iter().rev().find(|e| e.kind == "handoff")
    {
        sections.push(Section::new("handoff", vec![c.item(entry)]));
    }
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
            });
        }
    }
    missing.sort_by(|a, b| a.id.cmp(&b.id));
    if !missing.is_empty() {
        sections.push(Section::new("onboarding", missing));
    }
    Ok(Brief { sections })
}
