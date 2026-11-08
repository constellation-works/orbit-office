//! Handoff threads and open global handoffs.
use std::collections::BTreeMap;

use crate::Result;

use super::conversation::Conversation;
use super::entry::Entry;
use super::helpers::{cited_assignment, project_key, time_key};
use super::scope::SessionContext;

pub(crate) struct ThreadHandoff<'a> {
    pub(crate) key: String,
    pub(crate) conversation: &'a Conversation,
    pub(crate) entry: &'a Entry,
    pub(crate) at: (i64, String),
}

/// The thread a project handoff belongs to: `task:<id>` for the first
/// assignment id it cites, else its conversation's normalized project plus
/// orchestrator (separated by a NUL, which no frontmatter value contains).
pub(crate) fn thread_key(conversation: &Conversation, entry: &Entry) -> String {
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
pub(crate) fn thread_handoffs<'a>(
    conversations: &'a [Conversation],
) -> Result<Vec<ThreadHandoff<'a>>> {
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
pub(crate) struct GlobalHandoff<'a> {
    pub(crate) conversation: &'a Conversation,
    pub(crate) entry: &'a Entry,
    pub(crate) at: (i64, String),
}

/// Every open global handoff, newest first. A global is closed by a
/// `resolved` entry recorded at or after it that cites it (its full
/// `<source-id>#L<n>` from any conversation, or its local `#L<n>` later in
/// its own log); it expires when its `until` is at or before `now`.
pub(crate) fn open_globals<'a>(
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
pub(crate) fn closing<'a>(
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
pub(crate) fn scoped_project(context: &SessionContext) -> Option<String> {
    if context.manual {
        return None;
    }
    context
        .project
        .as_deref()
        .and_then(|project| project_key(Some(project)))
}
