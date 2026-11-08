//! The legacy handoff list (`office.session-brief` with `view: list` and
//! `legacy: true`, kept for the scribe v2 compatibility window): every
//! thread's current handoff, the global handoffs with their state, and the
//! paged history of project handoffs. Library reads only; nothing is
//! inferred. Without `legacy`, `view: list` returns the SCR cards feed.

use serde::Serialize;

use super::{
    Brief, Conversation, Entry, Item, Section, closing, conversations, project_key,
    thread_handoffs, thread_key, time_key,
};
use crate::Result;
use crate::domain::source::LibraryText;

/// Cap for the `threads` section: at most this many current threads.
pub const THREADS_LIMIT: usize = 200;
/// Cap for the list view's `global` section.
pub const GLOBALS_LIMIT: usize = 200;
/// Default and largest `history` page.
pub const HISTORY_PAGE_DEFAULT: usize = 50;
/// Largest `history` page a caller may ask for.
pub const HISTORY_PAGE_MAX: usize = 100;

/// What the list view was asked for. Filters narrow `history` only.
#[derive(Clone, Debug, Default)]
pub struct ListQuery {
    /// Project filter, normalized like a conversation's `project`.
    pub project: Option<String>,
    /// Orchestrator filter, compared exactly (the provider name).
    pub orchestrator: Option<String>,
    /// Thread filter: a `thread` value this view returned.
    pub thread: Option<String>,
    /// Also list expired and resolved global handoffs.
    pub include_closed: bool,
    /// First `history` index to return.
    pub offset: usize,
    /// `history` page size, 1 to [`HISTORY_PAGE_MAX`].
    pub limit: usize,
    /// The current instant (RFC 3339) from the Clock port, for `until`.
    pub now: Option<String>,
}

/// The `resolved` entry that closed a handoff.
#[derive(Debug, Serialize)]
pub struct Closing {
    /// `<source-id>#L<n>` of the resolved entry.
    pub id: String,
    /// Library-relative path with its `#L<n>` anchor.
    pub link: String,
    /// The resolved entry's text; null when its source is private.
    pub text: Option<String>,
    /// The resolved entry's recorded time.
    pub time: String,
}

/// Thread, scope and state of one listed handoff.
#[derive(Debug, Serialize)]
pub struct Listing {
    /// `project` or `global`.
    pub scope: &'static str,
    /// The thread key (`task:<id>`, else `project:<project>
    /// orchestrator:<orchestrator>`); null for a global handoff.
    pub thread: Option<String>,
    /// The conversation's normalized project.
    pub project: Option<String>,
    /// The conversation's orchestrator (provider name).
    pub orchestrator: Option<String>,
    /// The first Orbit task id the handoff cites, if any.
    pub assignment: Option<String>,
    /// The conversation's session id, for `claude --resume`.
    pub session_id: Option<String>,
    /// The conversation source's title; empty when the source is private.
    pub source_title: String,
    /// A later handoff in the thread (or in the same conversation) replaced it.
    pub superseded: bool,
    /// `open`, `resolved` (a `resolved` entry cites it) or, for a global,
    /// `expired` (its `until` is at or before now).
    pub state: &'static str,
    /// A global handoff's `until`, when set.
    pub until: Option<String>,
    /// The `resolved` entry that closed it, when `state` is `resolved`.
    pub resolved_by: Option<Closing>,
    /// The source is private: `text` and `source_title` are empty and no
    /// structured fields are returned, as private session content stays out
    /// of the app. Its cited task id, thread and anchors remain references.
    pub withheld: bool,
}

struct Row<'a> {
    conversation: &'a Conversation,
    entry: &'a Entry,
    at: (i64, String),
}

fn newest_first(rows: &mut [Row<'_>]) {
    rows.sort_by(|a, b| {
        (b.at.clone(), &b.conversation.id, b.entry.line).cmp(&(
            a.at.clone(),
            &a.conversation.id,
            a.entry.line,
        ))
    });
}

fn listed(
    all: &[Conversation],
    conversation: &Conversation,
    entry: &Entry,
    superseded: bool,
    expired: bool,
) -> Result<Item> {
    let scope = conversation.globals.get(&entry.line);
    let closed = closing(all, conversation, entry)?;
    let state = if closed.is_some() {
        "resolved"
    } else if expired {
        "expired"
    } else {
        "open"
    };
    let mut item = conversation.item(entry);
    if conversation.private {
        item.text = String::new();
        item.structured_handoff = None;
        item.legacy = None;
    }
    item.listing = Some(Listing {
        scope: if scope.is_some() { "global" } else { "project" },
        thread: scope
            .is_none()
            .then(|| thread_key(conversation, entry).replace('\u{0}', " ")),
        project: project_key(conversation.project.as_deref()),
        orchestrator: conversation.orchestrator.clone(),
        assignment: super::cited_assignment(&entry.text),
        session_id: conversation.session_id.clone(),
        source_title: if conversation.private {
            String::new()
        } else {
            conversation.title.clone()
        },
        superseded,
        state,
        until: scope.and_then(|scope| scope.until.clone()),
        resolved_by: closed.map(|(owner, resolved)| Closing {
            id: format!("{}#L{}", owner.id, resolved.line),
            link: format!("{}#L{}", owner.path, resolved.line),
            text: (!owner.private).then(|| resolved.text.clone()),
            time: resolved.time.clone(),
        }),
        withheld: conversation.private,
    });
    Ok(item)
}

fn capped(kind: &'static str, items: Vec<Item>, cap: usize) -> Section {
    let total = items.len();
    let mut section = Section::new(kind, items.into_iter().take(cap).collect());
    section.total = total;
    section.truncated = total > section.items.len();
    section
}

/// The list view: `threads` (each open thread's current handoff, newest
/// first), `global` (open global handoffs; with `include_closed`, expired
/// and resolved ones too, each with its state) and `history` (every project
/// handoff, newest first, filtered and paged). Always all three sections.
pub fn list(sources: &[LibraryText], query: &ListQuery) -> Result<Brief> {
    let all = conversations(sources)?;
    let now = query.now.as_deref().map(time_key).transpose()?;
    let heads = thread_handoffs(&all)?;
    let is_head = |conversation: &Conversation, entry: &Entry| {
        heads
            .iter()
            .any(|head| head.conversation.id == conversation.id && head.entry.line == entry.line)
    };

    let mut current: Vec<Row<'_>> = heads
        .iter()
        .map(|head| Row {
            conversation: head.conversation,
            entry: head.entry,
            at: head.at.clone(),
        })
        .collect();
    newest_first(&mut current);
    let mut threads = Vec::new();
    for row in &current {
        let item = listed(&all, row.conversation, row.entry, false, false)?;
        if item.listing.as_ref().is_some_and(|l| l.state == "open") {
            threads.push(item);
        }
    }

    let mut globals: Vec<Row<'_>> = Vec::new();
    let mut history: Vec<Row<'_>> = Vec::new();
    for conversation in &all {
        for entry in conversation.log.iter().filter(|e| e.kind == "handoff") {
            let row = Row {
                conversation,
                entry,
                at: time_key(&entry.time)?,
            };
            if conversation.globals.contains_key(&entry.line) {
                globals.push(row);
            } else {
                history.push(row);
            }
        }
    }
    newest_first(&mut globals);
    let mut global_items = Vec::new();
    for row in &globals {
        let until = row
            .conversation
            .globals
            .get(&row.entry.line)
            .and_then(|scope| scope.until.as_deref());
        let expired = match (until, now.as_ref()) {
            (Some(until), Some(now)) => time_key(until)? <= *now,
            _ => false,
        };
        let item = listed(&all, row.conversation, row.entry, false, expired)?;
        if query.include_closed || item.listing.as_ref().is_some_and(|l| l.state == "open") {
            global_items.push(item);
        }
    }

    let wanted_project = query.project.as_deref().and_then(|p| project_key(Some(p)));
    newest_first(&mut history);
    let mut matching = Vec::new();
    for row in &history {
        if let Some(project) = &wanted_project
            && project_key(row.conversation.project.as_deref()).as_ref() != Some(project)
        {
            continue;
        }
        if let Some(orchestrator) = &query.orchestrator
            && row.conversation.orchestrator.as_ref() != Some(orchestrator)
        {
            continue;
        }
        if let Some(thread) = &query.thread
            && thread_key(row.conversation, row.entry).replace('\u{0}', " ") != *thread
        {
            continue;
        }
        matching.push(row);
    }
    let total = matching.len();
    let mut page = Vec::new();
    for row in matching.iter().skip(query.offset).take(query.limit) {
        let superseded = !is_head(row.conversation, row.entry);
        page.push(listed(
            &all,
            row.conversation,
            row.entry,
            superseded,
            false,
        )?);
    }
    let mut history_section = Section::new("history", page);
    history_section.total = total;
    history_section.truncated = query.offset.saturating_add(history_section.items.len()) < total;
    history_section.offset = Some(query.offset);

    Ok(Brief {
        sections: vec![
            capped("threads", threads, THREADS_LIMIT),
            capped("global", global_items, GLOBALS_LIMIT),
            history_section,
        ],
    })
}
