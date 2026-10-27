//! Proposal signals: repetition, friction recurrence, staleness, gaps,
//! unused automation, new subject, unrecorded direction and declared
//! cadence (specs/proposals.md#signals).
//!
//! Pure and mechanical: callers turn Orbit and Library records into the
//! fact shapes below, and [`scan`] returns one candidate per fired signal
//! with the evidence ids that fired it. Thresholds come from Library
//! settings ([`SignalThresholds`]); changing one changes the result.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::settings::SignalThresholds;

/// One completed or open task, reduced to what the signals read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskFact {
    /// Orbit task id.
    pub id: String,
    /// Task title.
    pub title: String,
    /// Task description.
    pub description: String,
    /// Tags, including `owner:you` for user-owned work.
    pub tags: Vec<String>,
    /// Orbit status (`done`, `in-progress`, …).
    pub status: String,
    /// Days since completion; `None` when not completed.
    pub completed_days_ago: Option<u32>,
    /// The user had to correct an agent's output.
    pub user_corrected: bool,
    /// A runbook already covers this work.
    pub has_runbook: bool,
}

/// One friction record, reduced to what the signals read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrictionFact {
    /// Friction record id.
    pub id: String,
    /// What went wrong, as recorded.
    pub text: String,
    /// Explicit link to the same friction, when recorded.
    pub link: Option<String>,
}

/// One knowledge page, reduced to what the signals read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageFact {
    /// Library document id (`dom-…` or `rb-…`).
    pub id: String,
    /// Page title.
    pub title: String,
    /// Other names the subject goes by.
    pub aliases: Vec<String>,
    /// Page tags (`client:…`, `project:…`, `topic:…`).
    pub tags: Vec<String>,
    /// Days since `last_verified`; `None` when never verified.
    pub last_verified_days_ago: Option<u32>,
    /// Ids of newer sources/tasks citing this page after verification.
    pub cited_by: Vec<String>,
    /// A newer citing source is available for contradiction/extension review.
    pub has_newer_evidence: bool,
    /// Outgoing source citations from the page, including optional anchors.
    pub cites: Vec<String>,
}

/// One Library source, reduced to what the signals read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFact {
    /// Library source id (`src-…`).
    pub id: String,
    /// Participants named by the source.
    pub participants: Vec<String>,
    /// Source tags.
    pub tags: Vec<String>,
    /// Source body (direction and rhythm entries are read from it).
    pub body: String,
}

/// One auto-task, reduced to what the signals read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationFact {
    /// Automation id.
    pub id: String,
    /// What it delivers.
    pub name: String,
    /// Days since the user last opened its output; `None` if never.
    pub last_opened_days_ago: Option<u32>,
}

/// Every fact a scan reads, grouped by origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignalFacts {
    /// Orbit tasks.
    pub tasks: Vec<TaskFact>,
    /// Orbit friction records.
    pub frictions: Vec<FrictionFact>,
    /// Library knowledge pages.
    pub pages: Vec<PageFact>,
    /// Library sources.
    pub sources: Vec<SourceFact>,
    /// Auto-tasks.
    pub automation: Vec<AutomationFact>,
}

pub use super::proposal::ProposalKind;

/// One fired signal: the proposal it suggests and its evidence ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// Signal name (`repetition`, `friction_recurrence`, …).
    pub signal: &'static str,
    /// Proposal kind the signal proposes.
    pub kind: ProposalKind,
    /// Human-readable summary naming the clustered work or subject.
    pub summary: String,
    /// Evidence ids (task, friction, page, source or automation ids), sorted.
    pub evidence: Vec<String>,
}

/// Compute every signal over `facts` under `thresholds`.
///
/// Candidates are ordered by signal, then summary, so equal inputs give
/// equal outputs.
#[must_use]
pub fn scan(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    let mut out = Vec::new();
    out.extend(repetition(facts, thresholds));
    out.extend(friction_recurrence(facts, thresholds));
    out.extend(staleness(facts, thresholds));
    out.extend(gaps(facts));
    out.extend(unused_automation(facts, thresholds));
    out.extend(new_subject(facts, thresholds));
    out.extend(unrecorded_direction(facts));
    out.extend(declared_cadence(facts));
    out.sort_by(|a, b| (a.signal, &a.summary).cmp(&(b.signal, &b.summary)));
    out
}

/// Lowercase word tokens of at least three characters.
fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|t| t.chars().count() >= 3)
        .collect()
}

/// Jaccard similarity of two token sets; empty sets never match.
fn similar(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let shared = a.intersection(b).count();
    let total = a.union(b).count();
    // Above one half, without floats.
    total > 0 && shared * 2 > total
}

/// Connected groups of indices whose items pairwise-match by `matches`.
fn clusters<T>(items: &[T], matches: impl Fn(&T, &T) -> bool) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            if matches(&items[i], &items[j]) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..items.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    groups.into_values().collect()
}

fn repetition(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    let recent: Vec<&TaskFact> = facts
        .tasks
        .iter()
        .filter(|t| {
            t.status == "done"
                && t.tags.iter().any(|tag| tag == "owner:you")
                && t.completed_days_ago
                    .is_some_and(|days| days <= thresholds.window_days)
        })
        .collect();
    let token_sets: Vec<BTreeSet<String>> = recent
        .iter()
        .map(|t| tokens(&format!("{} {}", t.title, t.description)))
        .collect();
    clusters(&recent, |a, b| {
        // The same tags (beyond ownership) or similar wording clusters work.
        let idx = |t: &TaskFact| recent.iter().position(|r| r.id == t.id).unwrap_or(0);
        a.tags
            .iter()
            .any(|t| t.as_str() != "owner:you" && b.tags.contains(t))
            || similar(&token_sets[idx(a)], &token_sets[idx(b)])
    })
    .into_iter()
    .filter(|group| group.len() >= thresholds.repetition_count as usize)
    .map(|group| {
        let mut evidence: Vec<String> = group.iter().map(|&i| recent[i].id.clone()).collect();
        evidence.sort();
        Candidate {
            signal: "repetition",
            kind: ProposalKind::AutoTask,
            summary: format!(
                "{} similar completed tasks: {}",
                group.len(),
                recent[group[0]].title
            ),
            evidence,
        }
    })
    .collect()
}

fn friction_recurrence(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    let token_sets: Vec<BTreeSet<String>> =
        facts.frictions.iter().map(|f| tokens(&f.text)).collect();
    clusters(&facts.frictions, |a, b| {
        (a.link.is_some() && a.link == b.link)
            || similar(
                &token_sets[facts
                    .frictions
                    .iter()
                    .position(|f| f.id == a.id)
                    .unwrap_or(0)],
                &token_sets[facts
                    .frictions
                    .iter()
                    .position(|f| f.id == b.id)
                    .unwrap_or(0)],
            )
    })
    .into_iter()
    .filter(|group| group.len() >= thresholds.friction_count as usize)
    .map(|group| {
        let mut evidence: Vec<String> = group
            .iter()
            .map(|&i| facts.frictions[i].id.clone())
            .collect();
        evidence.sort();
        Candidate {
            signal: "friction_recurrence",
            kind: ProposalKind::RunbookFromFriction,
            summary: format!(
                "friction recorded {} times: {}",
                group.len(),
                facts.frictions[group[0]].text
            ),
            evidence,
        }
    })
    .collect()
}

fn staleness(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    facts
        .pages
        .iter()
        .filter(|p| {
            p.has_newer_evidence
                && !p.cited_by.is_empty()
                && p.last_verified_days_ago
                    .is_some_and(|days| days >= thresholds.stale_days)
        })
        .map(|p| {
            let mut evidence = vec![p.id.clone()];
            evidence.extend(p.cited_by.iter().cloned());
            evidence.sort();
            evidence.dedup();
            Candidate {
                signal: "staleness",
                kind: ProposalKind::KnowledgeUpdate,
                summary: format!("page {} may be stale", p.title),
                evidence,
            }
        })
        .collect()
}

fn gaps(facts: &SignalFacts) -> Vec<Candidate> {
    facts
        .tasks
        .iter()
        .filter(|t| t.status == "done" && t.user_corrected && !t.has_runbook)
        .map(|t| Candidate {
            signal: "gap",
            kind: ProposalKind::KnowledgeAdd,
            summary: format!("no runbook for corrected task: {}", t.title),
            evidence: vec![t.id.clone()],
        })
        .collect()
}

fn unused_automation(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    facts
        .automation
        .iter()
        .filter(|a| {
            a.last_opened_days_ago
                .is_none_or(|days| days >= thresholds.unused_days)
        })
        .map(|a| Candidate {
            signal: "unused_automation",
            kind: ProposalKind::Retire,
            summary: format!("automation unused: {}", a.name),
            evidence: vec![a.id.clone()],
        })
        .collect()
}

/// Subjects a source mentions: its participants and namespaced tags.
fn subjects(source: &SourceFact) -> Vec<String> {
    let mut out: Vec<String> = source.participants.clone();
    for tag in &source.tags {
        if let Some((kind, name)) = tag.split_once(':')
            && ["client", "project", "topic"].contains(&kind)
        {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

fn new_subject(facts: &SignalFacts, thresholds: &SignalThresholds) -> Vec<Candidate> {
    let covered: BTreeSet<String> = facts
        .pages
        .iter()
        .flat_map(|p| {
            let mut names = vec![p.title.to_lowercase()];
            names.extend(p.aliases.iter().map(|a| a.to_lowercase()));
            names.extend(p.tags.iter().map(|t| {
                t.split_once(':')
                    .map_or_else(|| t.to_lowercase(), |(_, name)| name.to_lowercase())
            }));
            names
        })
        .collect();
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for source in &facts.sources {
        for subject in subjects(source) {
            let key = subject.to_lowercase();
            if covered.contains(&key) {
                continue;
            }
            let entry = seen.entry(key).or_default();
            if !entry.contains(&source.id) {
                entry.push(source.id.clone());
            }
        }
    }
    seen.into_iter()
        .filter(|(_, ids)| ids.len() >= thresholds.new_subject_count as usize)
        .map(|(subject, mut ids)| {
            ids.sort();
            Candidate {
                signal: "new_subject",
                kind: ProposalKind::KnowledgeAdd,
                summary: format!("new subject not covered by a page: {subject}"),
                evidence: ids,
            }
        })
        .collect()
}

/// Entries of one `marker:` kind in a source body, trimmed.
fn entries(body: &str, marker: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', ' ']);
            // Conversation capture writes `[RFC3339] kind: text`.
            let line = line
                .strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .filter(|(stamp, _)| stamp.contains('T'))
                .map_or(line, |(_, rest)| rest.trim_start());
            line.strip_prefix(marker)
                .map(|rest| rest.trim().trim_matches('"').to_string())
        })
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn unrecorded_direction(facts: &SignalFacts) -> Vec<Candidate> {
    // Standing directions live on `dom-standing-directions`; a direction
    // is recorded when that page cites its source (spec wording).
    let recorded_sources: BTreeSet<&str> = facts
        .pages
        .iter()
        .filter(|p| p.id == "dom-standing-directions")
        .flat_map(|p| {
            p.cites
                .iter()
                .map(|cite| cite.split('#').next().unwrap_or(cite))
        })
        .collect();
    let mut out = Vec::new();
    for source in &facts.sources {
        for entry in entries(&source.body, "direction:") {
            if !recorded_sources.contains(source.id.as_str()) {
                out.push(Candidate {
                    signal: "unrecorded_direction",
                    kind: ProposalKind::KnowledgeUpdate,
                    summary: format!("unrecorded direction: {entry}"),
                    evidence: vec![source.id.clone()],
                });
            }
        }
    }
    out
}

fn declared_cadence(facts: &SignalFacts) -> Vec<Candidate> {
    let covered: BTreeSet<String> = facts
        .automation
        .iter()
        .map(|a| a.name.to_lowercase())
        .chain(facts.tasks.iter().map(|t| t.title.to_lowercase()))
        .collect();
    let mut out = Vec::new();
    for source in &facts.sources {
        for entry in entries(&source.body, "rhythm:") {
            let deliverable = entry
                .split(['—', '-', ':'])
                .next()
                .unwrap_or(&entry)
                .trim()
                .to_lowercase();
            if !covered.iter().any(|name| name.contains(&deliverable)) {
                out.push(Candidate {
                    signal: "declared_cadence",
                    kind: ProposalKind::AutoTask,
                    summary: format!("declared cadence with no automation: {entry}"),
                    evidence: vec![source.id.clone()],
                });
            }
        }
    }
    out
}
