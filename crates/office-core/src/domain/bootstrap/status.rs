//! Computed setup counts and gaps; no inference and no ambient clock.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::{PHASES, Snapshot, digest, normalize, subjects};
use crate::domain::metrics::{metadata, timestamp};
use crate::domain::source::video::refusal;
use crate::{Error, Result};

/// One mechanical gap, with the source ids that establish it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Gap {
    /// subject_without_page, procedure_without_runbook, owner_without_person,
    /// or policy_without_current_as_of.
    pub kind: String,
    /// Normalized subject, procedure, owner or policy name.
    pub subject: String,
    /// Distinct supporting source ids, sorted.
    pub sources: Vec<String>,
}

/// Progress for one phase; zero means observed empty, never an unavailable read.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Phase {
    /// docs, kt, team or process.
    pub phase: String,
    /// Captured sources, including those awaiting OCR.
    pub captured: usize,
    /// Sources with valid digest.json.
    pub digested: usize,
    /// Subject groups recorded as actively consolidating.
    pub consolidating: usize,
    /// Knowledge proposals awaiting a decision.
    pub proposed: usize,
    /// Knowledge proposals accepted (done).
    pub accepted: usize,
    /// Shared pages covering phase subjects or explicitly tagged for this phase.
    pub shared: usize,
    /// Knowledge proposals dismissed (rejected).
    pub dismissed: usize,
    /// Subjects deferred by the new-page cap.
    pub overflow_waiting: usize,
    /// Gaps supported by the current corpus.
    pub gaps: Vec<Gap>,
}

fn strings(value: Option<&Value>) -> Result<Vec<String>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    refusal(
                        "invalid_bootstrap_data",
                        "metadata lists must contain strings",
                    )
                })
            })
            .collect(),
        Some(_) => Err(refusal(
            "invalid_bootstrap_data",
            "metadata lists must be arrays",
        )),
    }
}
fn names(fields: &serde_json::Map<String, Value>) -> Result<BTreeSet<String>> {
    let mut names = strings(fields.get("aliases"))?
        .into_iter()
        .map(|s| normalize(&s))
        .collect::<BTreeSet<_>>();
    if let Some(title) = fields.get("title").and_then(Value::as_str) {
        names.insert(normalize(title));
    }
    names.remove("");
    Ok(names)
}

/// Validate the complete Orbit response, including truncation, before counting.
pub fn tasks(value: &Value) -> Result<&[Value]> {
    let rows = value
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            refusal(
                "invalid_bootstrap_data",
                "orbit.task.list.tasks must be an array",
            )
        })?;
    if value.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(refusal(
            "task_list_truncated",
            "bootstrap status requires the complete task list",
        ));
    }
    if value.get("total").and_then(Value::as_u64) != u64::try_from(rows.len()).ok() {
        return Err(refusal(
            "invalid_bootstrap_data",
            "orbit.task.list.total does not match the returned tasks",
        ));
    }
    let mut ids = BTreeSet::new();
    for row in rows {
        if row
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| id.is_empty() || !ids.insert(id))
            || row.get("status").and_then(Value::as_str).is_none()
            || !row.get("tags").is_some_and(Value::is_array)
        {
            return Err(refusal(
                "invalid_bootstrap_data",
                "Orbit tasks need unique ids, status and tags",
            ));
        }
        strings(row.get("tags"))?;
    }
    Ok(rows)
}

/// Compute phase reports against full local files and the authoritative proposal tasks.
pub fn compute(
    snapshot: &Snapshot,
    task_rows: &[Value],
    now: i64,
    stale_days: u32,
) -> Result<Vec<Phase>> {
    let mut page_names = Vec::new();
    for (path, markdown) in &snapshot.pages {
        let fields = metadata(markdown)?;
        page_names.push((path, fields.clone(), names(&fields)?));
    }
    let mut all_subjects = Vec::new();
    for source in &snapshot.sources {
        source.fields()?;
        if let Some(value) = &source.digest {
            digest(value)?;
            all_subjects.extend(value["subjects"].as_array().into_iter().flatten().cloned());
        }
    }
    // Page aliases also bridge digest groups, so identity is shared by every check.
    for (_, fields, _) in &page_names {
        all_subjects.push(serde_json::json!({"name":fields.get("title"),"aliases":fields.get("aliases").unwrap_or(&serde_json::json!([]))}));
    }
    let groups = subjects(all_subjects.into_iter());
    let identity = |name: &str| {
        let name = normalize(name);
        groups
            .iter()
            .find(|group| group.contains(&name))
            .and_then(|group| group.first())
            .cloned()
            .unwrap_or(name)
    };
    let covered = |subject: &str, template: Option<&str>| {
        page_names.iter().any(|(path, fields, names)| {
            let kind = match template {
                None => true,
                Some("runbook") => {
                    crate::domain::knowledge::local_document_path(path)
                        .starts_with("knowledge/runbooks/")
                        || fields.get("template").and_then(Value::as_str) == Some("runbook")
                }
                Some(template) => fields.get("template").and_then(Value::as_str) == Some(template),
            };
            kind && names.iter().any(|name| identity(name) == identity(subject))
        })
    };
    let current = |value: Option<&Value>, evidence: Option<i64>| {
        let date = value.and_then(Value::as_str).and_then(date_timestamp);
        date.is_some_and(|date| {
            date <= now
                && now.saturating_sub(date) <= i64::from(stale_days) * 86400
                && evidence.is_none_or(|newer| date >= newer)
        })
    };
    let mut result = Vec::new();
    for phase_name in PHASES {
        let mut report = Phase {
            phase: phase_name.into(),
            ..Phase::default()
        };
        let mut subject_sources: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut gaps: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
        for source in &snapshot.sources {
            if !source.in_phase(phase_name)? {
                continue;
            }
            report.captured += 1;
            let fields = source.fields()?;
            let id = fields["id"].as_str().unwrap_or_default();
            let Some(value) = &source.digest else {
                continue;
            };
            report.digested += 1;
            for subject in value["subjects"].as_array().into_iter().flatten() {
                let name = subject["name"].as_str().unwrap_or_default();
                let key = identity(name);
                subject_sources
                    .entry(key.clone())
                    .or_default()
                    .insert(id.into());
                let owner = subject["template"] == "person"
                    && subject["role"].as_str().is_some_and(|role| {
                        normalize(role)
                            .split_whitespace()
                            .any(|word| word == "owner")
                    });
                if owner && !covered(name, Some("person")) {
                    gaps.entry(("owner_without_person".into(), key))
                        .or_default()
                        .insert(id.into());
                }
            }
            if phase_name == "process" {
                for procedure in value["procedures"].as_array().into_iter().flatten() {
                    let title = procedure["title"].as_str().unwrap_or_default();
                    if !covered(title, Some("runbook")) {
                        gaps.entry(("procedure_without_runbook".into(), identity(title)))
                            .or_default()
                            .insert(id.into());
                    }
                }
            }
            if value["kind_hint"].as_str().is_some_and(|kind| {
                normalize(kind)
                    .split_whitespace()
                    .any(|word| word == "policy")
            }) {
                let title = fields.get("title").and_then(Value::as_str).unwrap_or(id);
                if !current(value.get("as_of"), None) {
                    gaps.entry(("policy_without_current_as_of".into(), identity(title)))
                        .or_default()
                        .insert(id.into());
                }
            }
        }
        for (path, fields, names) in &page_names {
            if path.starts_with("shared/")
                && (strings(fields.get("tags"))?.contains(&format!("bootstrap:{phase_name}"))
                    || names
                        .iter()
                        .any(|name| subject_sources.contains_key(&identity(name))))
            {
                report.shared += 1;
            }
        }
        for (subject, ids) in subject_sources {
            if ids.len() >= 3 && !covered(&subject, None) {
                gaps.insert(("subject_without_page".into(), subject), ids);
            }
        }
        // Policies on maintained pages must be dated and at least as new as their cited digest.
        for (_, fields, _) in &page_names {
            if !strings(fields.get("tags"))?
                .iter()
                .any(|tag| tag == "policy")
            {
                continue;
            }
            let cites = strings(fields.get("cites"))?
                .into_iter()
                .map(|cite| cite.split('#').next().unwrap_or_default().to_string())
                .collect::<BTreeSet<_>>();
            let relevant = snapshot
                .sources
                .iter()
                .filter(|source| {
                    source.in_phase(phase_name).unwrap_or(false)
                        && source
                            .fields()
                            .ok()
                            .is_some_and(|f| f["id"].as_str().is_some_and(|id| cites.contains(id)))
                })
                .collect::<Vec<_>>();
            let tagged = strings(fields.get("tags"))?.contains(&format!("bootstrap:{phase_name}"));
            if relevant.is_empty() && !tagged {
                continue;
            }
            let latest = relevant
                .iter()
                .filter_map(|s| date_timestamp(s.digest.as_ref()?.get("as_of")?.as_str()?))
                .max();
            if !current(fields.get("as_of"), latest) {
                let title = fields
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::InvalidArgument("policy page needs a title".into()))?;
                gaps.entry(("policy_without_current_as_of".into(), identity(title)))
                    .or_default()
                    .extend(cites);
            }
        }
        for row in task_rows {
            let tags = strings(row.get("tags"))?;
            if !tags.contains(&format!("bootstrap:{phase_name}"))
                || !tags.contains(&"proposal".into())
                || !tags.iter().any(|tag| {
                    tag == "proposal:knowledge_add" || tag == "proposal:knowledge_update"
                })
            {
                continue;
            }
            match row["status"].as_str().unwrap_or_default() {
                "done" => report.accepted += 1,
                "rejected" => report.dismissed += 1,
                "proposed" | "backlog" | "in-progress" | "review" | "blocked" | "someday" => {
                    report.proposed += 1
                }
                "archived" => {}
                _ => {
                    return Err(refusal(
                        "invalid_bootstrap_data",
                        "proposal has an unsupported lifecycle status",
                    ));
                }
            }
        }
        if let Some(batch) = snapshot.batches.get(phase_name) {
            report.consolidating = batch.consolidating.len();
            report.overflow_waiting = batch.overflow.len();
        }
        report.gaps = gaps
            .into_iter()
            .map(|((kind, subject), sources)| Gap {
                kind,
                subject,
                sources: sources.into_iter().collect(),
            })
            .collect();
        result.push(report);
    }
    Ok(result)
}

fn date_timestamp(value: &str) -> Option<i64> {
    if value.len() == 10 {
        timestamp(&format!("{value}T00:00:00Z"))
    } else {
        timestamp(value)
    }
}
