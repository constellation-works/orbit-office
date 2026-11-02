//! Model-free candidate selection and recency weighting for Orbit. All
//! file access and FTS5 ranking cross the Library port. Contradiction
//! to-dos arrive as an already-read `orbit.task.list` value; this module
//! does not call Orbit.
mod contract;
mod render;

use super::decisions::{DecidedBy, Decision, Ledger, parse_entry_id};
use super::sha256_hex;
use crate::Result;
use crate::domain::knowledge::frontmatter;
use crate::ports::LibraryText;
use chrono::{DateTime, FixedOffset};
pub use contract::Request;
use contract::{CONTRACT, RESPONSE_BYTES};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct Candidate {
    decision: Decision,
    at: DateTime<FixedOffset>,
    ledger: String,
    line: usize,
    digest: String,
    group: String,
    reversed_by: Vec<String>,
    withheld: bool,
}

/// Readable eligible evidence plus the text to rank through the Library.
pub struct Pool {
    /// `(entry id, question/options/chosen/because)` for eligible entries.
    pub documents: Vec<(String, String)>,
    entries: Vec<Candidate>,
    omissions: BTreeSet<String>,
}

fn ledger_id(path: &str) -> String {
    format!(
        "decisions-{}",
        path.rsplit('/')
            .next()
            .unwrap_or(path)
            .trim_end_matches(".md")
    )
}

fn group_members(start: &str, edges: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![start.to_owned()];
    while let Some(id) = pending.pop() {
        if seen.insert(id.clone()) {
            pending.extend(edges.get(&id).into_iter().flatten().cloned());
        }
    }
    seen
}

impl Pool {
    /// Parse each ledger independently, naming unreadable/invalid files.
    /// Historical relationships use only evidence strictly before as_of.
    #[must_use]
    pub fn read(request: &Request, files: &[LibraryText], private: bool) -> Self {
        let mut pool = Self {
            documents: Vec::new(),
            entries: Vec::new(),
            omissions: BTreeSet::new(),
        };
        let mut rows = Vec::new();
        for file in files {
            let parsed = file.markdown.as_deref().and_then(|text| {
                let ledger = Ledger::parse(&file.path, text).ok()?;
                if ledger
                    .entries
                    .iter()
                    .any(|entry| entry.decision.validate().is_err())
                {
                    return None;
                }
                let (header, body) = frontmatter::split(text).ok()?;
                let header = frontmatter::parse(header).ok()?;
                let ledger_private = header
                    .iter()
                    .any(|(key, value)| key == "sensitivity" && value.as_str() == Some("private"));
                Some((ledger, body, ledger_private))
            });
            let Some((ledger, body, ledger_private)) = parsed else {
                pool.omissions.insert(ledger_id(&file.path));
                continue;
            };
            let lines: Vec<&str> = body.lines().collect();
            for entry in &ledger.entries {
                let decision = &entry.decision;
                if let Ok(at) = DateTime::parse_from_rfc3339(&decision.decided_at)
                    && at < request.as_of
                {
                    rows.push(Candidate {
                        decision: decision.clone(),
                        at,
                        ledger: ledger.id(),
                        line: entry.line,
                        digest: format!("sha256:{}", sha256_hex(lines[entry.line - 1].as_bytes())),
                        group: decision.id.clone(),
                        reversed_by: Vec::new(),
                        withheld: private || ledger_private,
                    });
                }
            }
        }
        let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ingested = BTreeSet::new();
        let mut confirmed = BTreeSet::new();
        let mut reversals: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in &rows {
            let d = &row.decision;
            if let Some(target) = &d.same_as {
                edges.entry(d.id.clone()).or_default().push(target.clone());
                edges.entry(target.clone()).or_default().push(d.id.clone());
            }
            if d.source.starts_with("orbit-decision:") {
                ingested.insert(d.id.clone());
            }
            if d.decided_by == DecidedBy::Human
                && !d.private
                && !row.withheld
                && let Some(target) = &d.confirms
            {
                confirmed.insert(target.clone());
            }
            if let Some(target) = &d.reverses {
                reversals
                    .entry(target.clone())
                    .or_default()
                    .push(d.id.clone());
            }
        }
        let mut groups = BTreeMap::new();
        let mut excluded = BTreeSet::new();
        for row in &rows {
            if groups.contains_key(&row.decision.id) {
                continue;
            }
            let members = group_members(&row.decision.id, &edges);
            let canonical = members
                .first()
                .cloned()
                .unwrap_or_else(|| row.decision.id.clone());
            if members.iter().any(|id| ingested.contains(id)) {
                excluded.extend(members.iter().cloned());
            }
            for id in members {
                groups.insert(id, canonical.clone());
            }
        }
        // A same_as component is one decision: reversing any member must
        // flag the representative too, rather than resurrecting its twin.
        let mut group_reversals: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (target, reversing) in reversals {
            let group = groups.get(&target).cloned().unwrap_or(target);
            group_reversals.entry(group).or_default().extend(reversing);
        }
        for mut row in rows {
            let d = &row.decision;
            let in_scope = d.scope == "global"
                || d.scope
                    .strip_prefix("project:")
                    .is_some_and(|project| format!("ws_{project}") == request.workspace_id);
            let human = match d.decided_by {
                DecidedBy::Human => true,
                DecidedBy::Model => confirmed.contains(&d.id),
                DecidedBy::Agent => false,
            };
            if d.private
                || row.withheld
                || !in_scope
                || excluded.contains(&d.id)
                || !human
                || d.confirms.is_some()
                || d.orbit.as_ref().and_then(|o| o.gate.as_deref()) == Some("model_answer_review")
            {
                continue;
            }
            row.group = groups.get(&d.id).cloned().unwrap_or_else(|| d.id.clone());
            row.reversed_by = group_reversals.get(&row.group).cloned().unwrap_or_default();
            pool.documents.push((d.id.clone(), render::lexical_text(d)));
            pool.entries.push(row);
        }
        pool
    }
}

/// Open contradiction to-dos, or a read that did not return a task list.
pub enum ConflictList {
    /// The callback failed, or the value was not a task-list envelope.
    Unread,
    /// The `orbit.task.list` JSON value.
    Listed(Value),
}

/// To-dos Office treats as still open. `done`, `archived` and `rejected`
/// have left the user's queue.
pub const OPEN_TODO: &[&str] = &[
    "proposed",
    "backlog",
    "in-progress",
    "review",
    "blocked",
    "someday",
];

struct ConflictPass {
    /// Eligible entry id to the partner ids still unresolved.
    partners: BTreeMap<String, BTreeSet<String>>,
    unread: bool,
}

/// A reversal already in the `as_of` snapshot resolves the conflict: the
/// routine files only current entries, so `reversed_by` here happened since.
fn read_conflicts(pool: &Pool, conflicts: ConflictList) -> ConflictPass {
    let ConflictList::Listed(value) = conflicts else {
        return ConflictPass {
            partners: BTreeMap::new(),
            unread: true,
        };
    };
    let Some(tasks) = value.get("tasks").and_then(Value::as_array) else {
        return ConflictPass {
            partners: BTreeMap::new(),
            unread: true,
        };
    };
    let complete = value.get("truncated").and_then(Value::as_bool) == Some(false)
        && value.get("total").and_then(Value::as_u64) == u64::try_from(tasks.len()).ok();
    let mut by_id = BTreeMap::new();
    for entry in &pool.entries {
        by_id.insert(entry.decision.id.as_str(), entry);
    }
    let mut partners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for task in tasks {
        let Some((left_id, right_id)) = conflicting_pair(task) else {
            continue;
        };
        let Some(left) = by_id.get(left_id.as_str()).copied() else {
            continue;
        };
        let Some(right) = by_id.get(right_id.as_str()).copied() else {
            continue;
        };
        if left.group == right.group
            || !left.reversed_by.is_empty()
            || !right.reversed_by.is_empty()
        {
            continue;
        }
        partners
            .entry(left_id.clone())
            .or_default()
            .insert(right_id.clone());
        partners.entry(right_id).or_default().insert(left_id);
    }
    ConflictPass {
        partners,
        unread: !complete,
    }
}

/// The two conflicting ledger entry ids of an open contradiction to-do
/// (`owner:you`, `office-scribe-contradictions`, a final `Conflicting
/// entries: <a> <b>` line), or `None` for any other task row.
#[must_use]
pub fn conflicting_pair(task: &Value) -> Option<(String, String)> {
    let status = task.get("status").and_then(Value::as_str)?;
    if !OPEN_TODO.contains(&status) {
        return None;
    }
    let tags = task.get("tags").and_then(Value::as_array)?;
    let tagged = |name: &str| tags.iter().any(|tag| tag.as_str() == Some(name));
    if !tagged("owner:you") || !tagged("office-scribe-contradictions") {
        return None;
    }
    let description = task.get("description").and_then(Value::as_str)?;
    let line = description
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())?
        .trim();
    let rest = line.strip_prefix("Conflicting entries: ")?;
    let mut ids = rest.split_whitespace();
    let left = ids.next()?;
    let right = ids.next()?;
    if ids.next().is_some()
        || left == right
        || parse_entry_id(left).is_none()
        || parse_entry_id(right).is_none()
    {
        return None;
    }
    Some((left.to_owned(), right.to_owned()))
}

/// Partners count against `k` and need no lexical overlap. The ranked
/// entry and its reversals keep the room they already took; a partner
/// that does not fit is dropped and the caller sets `truncated`.
fn pull_conflict_partners<'a>(
    closure: &mut Vec<&'a Candidate>,
    partners: &BTreeMap<String, BTreeSet<String>>,
    pool: &'a Pool,
    space: usize,
    truncated: &mut bool,
    selected: &[&'a Candidate],
) {
    if partners.is_empty() || closure.is_empty() {
        return;
    }
    let mut groups: BTreeSet<&str> = selected.iter().map(|row| row.group.as_str()).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for row in closure.iter() {
        groups.insert(row.group.as_str());
        seen.insert(row.decision.id.clone());
    }
    let mut pending: Vec<&str> = closure.iter().map(|row| row.decision.id.as_str()).collect();
    let mut added = Vec::new();
    while let Some(id) = pending.pop() {
        let Some(ids) = partners.get(id) else {
            continue;
        };
        for partner_id in ids {
            if !seen.insert(partner_id.clone()) {
                continue;
            }
            let Some(partner) = pool
                .entries
                .iter()
                .find(|entry| entry.decision.id == *partner_id)
            else {
                continue;
            };
            if !groups.insert(partner.group.as_str()) {
                continue;
            }
            added.push(partner);
            pending.push(partner.decision.id.as_str());
        }
    }
    added.sort_by(|left, right| left.decision.id.cmp(&right.decision.id));
    let room = space.saturating_sub(closure.len());
    if added.len() > room {
        *truncated = true;
        added.truncate(room);
    }
    closure.extend(added);
}

/// Rank eligible, lexically matching groups; reserve space for reversal
/// evidence and unresolved conflict partners, then enforce wire bounds
/// without returning scores or choices.
pub fn response(
    request: &Request,
    pool: &Pool,
    scores: &BTreeMap<String, f64>,
    half_life: u32,
    conflicts: ConflictList,
) -> Result<Value> {
    let score = |row: &Candidate| {
        let d = &row.decision;
        let workspace = if d.context.workspace.as_deref() == Some(&request.workspace_id) {
            1.0
        } else if d.scope == "global" {
            0.5
        } else {
            0.0
        };
        let category = if d.orbit.as_ref().and_then(|o| o.category.as_deref())
            == Some(&request.request.category)
        {
            0.5
        } else {
            0.0
        };
        let age = request
            .as_of
            .signed_duration_since(row.at)
            .to_std()
            .map_or(0.0, |d| d.as_secs_f64() / 86_400.0);
        (workspace + scores.get(&d.id).copied().unwrap_or_default() + category)
            * 0.5_f64.powf(age / f64::from(half_life))
    };
    let mut ranked: Vec<&Candidate> = pool
        .entries
        .iter()
        .filter(|row| {
            scores
                .get(&row.decision.id)
                .is_some_and(|s| s.is_finite() && *s > 0.0)
        })
        .collect();
    ranked.sort_by(|a, b| {
        score(b)
            .total_cmp(&score(a))
            .then_with(|| b.at.cmp(&a.at))
            .then_with(|| a.decision.id.cmp(&b.decision.id))
    });
    let mut groups = BTreeSet::new();
    ranked.retain(|row| groups.insert(row.group.clone()));
    let conflict_pass = read_conflicts(pool, conflicts);
    let mut selected: Vec<&Candidate> = Vec::new();
    let mut omissions = pool.omissions.clone();
    if conflict_pass.unread {
        omissions.insert("conflicts_unread".to_owned());
    }
    let mut truncated = false;
    for row in ranked {
        if selected.iter().any(|r| r.group == row.group) {
            continue;
        }
        if selected.len() == request.k {
            break;
        }
        let mut closure = Vec::new();
        let mut pending = vec![row];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(&current.decision.id) {
                continue;
            }
            closure.push(current);
            for id in current.reversed_by.iter().rev() {
                match pool.entries.iter().find(|entry| entry.decision.id == *id) {
                    Some(reversing) => pending.push(reversing),
                    None => {
                        omissions.insert(current.ledger.clone());
                    }
                }
            }
        }
        let mut closure_groups = BTreeSet::new();
        closure.retain(|item| {
            !selected.iter().any(|s| s.group == item.group) && closure_groups.insert(&item.group)
        });
        let space = request.k - selected.len();
        if closure.len() > space {
            // Prefer the newer reversal over an old choice with no room
            // for both (including k=1). Mark the evidence gap explicitly.
            truncated = true;
            closure.drain(..closure.len() - space);
        }
        pull_conflict_partners(
            &mut closure,
            &conflict_pass.partners,
            pool,
            space,
            &mut truncated,
            &selected,
        );
        selected.extend(closure);
    }
    if omissions.len() > 32 {
        truncated = true;
    }
    let mut omissions: Vec<String> = omissions.into_iter().take(32).collect();
    if conflict_pass.unread && !omissions.iter().any(|id| id == "conflicts_unread") {
        match omissions.last_mut() {
            Some(slot) => *slot = "conflicts_unread".to_owned(),
            None => omissions.push("conflicts_unread".to_owned()),
        }
        truncated = true;
    }
    let mut precedents = Vec::new();
    for row in selected {
        let partners = conflict_pass
            .partners
            .get(&row.decision.id)
            .map(|ids| ids.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        precedents.push(render::precedent(request, row, &partners, &mut truncated));
    }
    loop {
        let out = json!({"contract": CONTRACT, "precedents": precedents, "omissions": omissions, "truncated":truncated});
        if serde_json::to_vec(&out)
            .map_err(|_| contract::invalid("response cannot be encoded"))?
            .len()
            <= RESPONSE_BYTES
        {
            return Ok(out);
        }
        precedents.pop();
        truncated = true;
    }
}
