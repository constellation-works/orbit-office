//! Brief assembly from read-only port snapshots.
use std::collections::BTreeMap;

use serde_json::Value;

use crate::Result;
use crate::domain::artifact::Artifact;
use crate::domain::knowledge::Page;
use crate::domain::source::LibraryText;
use crate::domain::template::Template;

use super::conversation::{Conversation, conversations};
use super::handoff_sections::handoff_sections;
use super::helpers::{cited_assignment, markdown, project_key, time_key};
use super::item::{Brief, Item, Section};
use super::scope::SessionContext;
use super::scr;
use super::tasks::{TaskInventory, task_sections};
use super::threads::scoped_project;

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
