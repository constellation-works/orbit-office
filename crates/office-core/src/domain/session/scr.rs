//! The v2 SCR as a handoff (specs/sessions.md, "Scribe v2: the latest SCR
//! is the handoff"): parse the Library's v2 SCR files, select the latest
//! per scope and render one as a brief item. Legacy handoff entries stay
//! the fallback for a scope with no SCR.

use std::collections::BTreeMap;

use super::{Item, cited_assignment, cites, invalid, project_key, time_key};
use crate::Result;
use crate::domain::source::LibraryText;
use crate::domain::source::conversation::scribe::v2::{self, Scr};

/// One v2 SCR keyed by its `updated` instant, for newest-first selection.
pub struct ScrHandoff {
    /// The parsed SCR.
    pub scr: Scr,
    /// `front.updated` as a sortable instant.
    pub at: (i64, String),
}

fn scr_id(scr: &Scr) -> String {
    if !scr.front.id.trim().is_empty() {
        return scr.front.id.clone();
    }
    scr.path
        .rsplit('/')
        .next()
        .unwrap_or(&scr.path)
        .strip_suffix(".md")
        .unwrap_or(&scr.path)
        .to_string()
}

/// Render the SCR's rewritten sections as the handoff text: the title and
/// `updated`, then Summary (keeping its `(scribe)` marker), Goals, Outcome
/// and Blockers and Next, in that order.
fn render(scr: &Scr) -> String {
    let title = scr
        .front
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map_or_else(|| scr_id(scr), str::to_string);
    let summary = if scr.sections.summary.starts_with("(scribe)") {
        scr.sections.summary.clone()
    } else if scr.sections.summary.is_empty() {
        "(scribe)".to_string()
    } else {
        format!("(scribe) {}", scr.sections.summary)
    };
    let list = |items: &[String]| {
        if items.is_empty() {
            "- None recorded.".to_string()
        } else {
            items
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    format!(
        "{title} ({})\nSummary: {summary}\nGoals:\n{}\nOutcome and Blockers:\n{}\nNext:\n{}",
        scr.front.updated,
        list(&scr.sections.goals),
        list(&scr.sections.outcome),
        list(&scr.sections.next),
    )
}

impl ScrHandoff {
    /// The SCR id (its front-matter id, else its file stem).
    #[must_use]
    pub fn id(&self) -> String {
        scr_id(&self.scr)
    }

    /// The normalized project of a project-scoped SCR; `None` for global.
    #[must_use]
    pub fn project(&self) -> Option<String> {
        if self.scr.front.scope == "project" {
            project_key(self.scr.front.project.as_deref())
        } else {
            None
        }
    }

    /// Whether this SCR is global-scoped.
    #[must_use]
    pub fn is_global(&self) -> bool {
        self.scr.front.scope == "global"
    }

    /// The brief item: the SCR id as its link target id, its Library path
    /// as the link, `updated` as its time and the rendered sections.
    #[must_use]
    pub fn item(&self) -> Item {
        Item {
            id: self.id(),
            link: self.scr.path.clone(),
            text: render(&self.scr),
            time: Some(self.scr.front.updated.clone()),
            entry_kind: Some("handoff".into()),
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

    /// Whether any Next item cites `task` as a complete task id.
    #[must_use]
    pub fn next_cites(&self, task: &str) -> bool {
        self.scr.sections.next.iter().any(|item| {
            cited_assignment(item).as_deref() == Some(task)
                || item
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                    .any(|token| token == task)
        })
    }

    /// Whether the rendered sections cite `anchor` (`<source-id>#L<n>`) as a
    /// whole token, as the legacy global-citation rule requires.
    #[must_use]
    pub fn cites_anchor(&self, anchor: &str) -> bool {
        cites(&render(&self.scr), anchor)
    }
}

/// Parse every v2 SCR among `files` (v1 files are skipped), newest first
/// by `updated` (ties by descending id). A v2 SCR whose scope, project or
/// `updated` cannot be read refuses the brief: unknown evidence never
/// looks like no handoff.
pub fn parse(files: &[LibraryText]) -> Result<Vec<ScrHandoff>> {
    let mut parsed = Vec::new();
    for scr in v2::v2_files(files)? {
        match scr.front.scope.as_str() {
            "global" => {}
            "project" => {
                if scr
                    .front
                    .project
                    .as_deref()
                    .is_none_or(|project| project.trim().is_empty())
                {
                    return Err(invalid("SCR project"));
                }
            }
            _ => return Err(invalid("SCR scope")),
        }
        let at = time_key(&scr.front.updated)?;
        parsed.push(ScrHandoff { scr, at });
    }
    parsed.sort_by_key(|scr| std::cmp::Reverse((scr.at.clone(), scr.id())));
    Ok(parsed)
}

/// The latest global SCR, if any.
#[must_use]
pub fn latest_global(scrs: &[ScrHandoff]) -> Option<&ScrHandoff> {
    scrs.iter().find(|scr| scr.is_global())
}

/// Every SCR in `project`, newest first.
#[must_use]
pub fn in_project<'a>(scrs: &'a [ScrHandoff], project: &str) -> Vec<&'a ScrHandoff> {
    scrs.iter()
        .filter(|scr| scr.project().as_deref() == Some(project))
        .collect()
}

/// The latest SCR per project, keyed by normalized project.
#[must_use]
pub fn latest_per_project(scrs: &[ScrHandoff]) -> BTreeMap<String, &ScrHandoff> {
    let mut latest = BTreeMap::new();
    for scr in scrs {
        if let Some(project) = scr.project() {
            latest.entry(project).or_insert(scr);
        }
    }
    latest
}
