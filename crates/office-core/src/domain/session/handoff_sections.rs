//! The handoff, global, projects and other-sessions sections.
use std::collections::BTreeMap;

use crate::Result;

use super::conversation::Conversation;
use super::entry::Entry;
use super::helpers::{project_key, time_key};
use super::item::{Item, Section};
use super::scope::{
    GLOBAL_HANDOFFS_LIMIT, OTHER_SESSIONS_LIMIT, OTHER_SESSIONS_WINDOW_SECONDS, PROJECTS_LIMIT,
    SessionContext,
};
use super::scr;
use super::threads::{GlobalHandoff, ThreadHandoff, open_globals, scoped_project, thread_handoffs};

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
pub(crate) fn handoff_sections(
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
