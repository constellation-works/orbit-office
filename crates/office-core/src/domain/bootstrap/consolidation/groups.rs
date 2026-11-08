//! Transitive identity groups from a confirmed batch and earlier-phase context.
use std::collections::{BTreeMap, BTreeSet};

use office_common::Sensitivity;
use serde_json::Value;

use super::super::{Batch, PHASES, Snapshot, digest, normalize};
use super::evidence::{
    candidate_subjects, cited, complete, date_seconds, names, overlaps, page_names, procedure,
    template,
};
use super::group::Group;
use crate::Result;
use crate::domain::knowledge::{Page, PageStatus};
use crate::domain::source::video::refusal;
use crate::domain::template::Template;

/// Build transitive identity groups from the confirmed batch and earlier-phase context.
/// Existing aliases bridge subjects; page matching is exact first, then overlap, id sorted.
/// Only groups supported by at least one source in the confirmed batch are candidates.
pub fn groups(
    snapshot: &Snapshot,
    batch: &Batch,
    templates: &BTreeMap<String, Template>,
) -> Result<Vec<Group>> {
    let phase_index = PHASES
        .iter()
        .position(|phase| *phase == batch.plan.phase)
        .ok_or_else(|| refusal("invalid_bootstrap_data", "unknown batch phase"))?;
    let mut pages = snapshot
        .pages
        .values()
        .map(|markdown| Ok((Page::from_markdown(markdown)?, markdown.clone())))
        .collect::<Result<Vec<_>>>()?;
    pages.retain(|(page, _)| page.status != PageStatus::Retired);
    pages.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let mut candidates = Vec::<Group>::new();
    let mut found = BTreeSet::new();
    for source in &snapshot.sources {
        let fields = source.fields()?;
        let id = fields["id"].as_str().unwrap_or_default();
        let in_batch = batch.plan.sources.iter().any(|s| s == id);
        let earlier = PHASES[..phase_index]
            .iter()
            .any(|phase| source.in_phase(phase).unwrap_or(false));
        if !in_batch && !earlier && !source.in_phase(&batch.plan.phase)? {
            continue;
        }
        let Some(value) = &source.digest else {
            if in_batch {
                return Err(refusal(
                    "digests_incomplete",
                    "finish every confirmed batch digest before consolidation",
                ));
            }
            continue;
        };
        found.insert(id.to_string());
        digest(value)?;
        let date = value
            .get("as_of")
            .filter(|v| date_seconds(v).is_some())
            .cloned()
            .unwrap_or_else(|| fields.get("captured_at").cloned().unwrap_or(Value::Null));
        let procedures = value["procedures"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| procedure(p, source, id, &date))
            .collect::<Vec<_>>();
        let mut local = Vec::<Group>::new();
        let mut subjects = candidate_subjects(value, &procedures);
        subjects.sort_by_key(|v| {
            (
                normalize(v["name"].as_str().unwrap_or_default()),
                v.to_string(),
            )
        });
        for subject in subjects {
            let names = names(&subject);
            if names.is_empty() {
                continue;
            }
            let name = subject["name"].as_str().unwrap_or_default();
            let kind = subject["template"].as_str().unwrap_or_default();
            template(templates, kind)?;
            local.push(Group {
                key: normalize(name),
                title: name.into(),
                names,
                template: kind.into(),
                sources: BTreeSet::from([id.into()]),
                claims: vec![],
                procedures: vec![],
                terms: vec![],
                existing: None,
                private: fields.get("sensitivity").and_then(Value::as_str) == Some("private"),
            });
        }
        for group in &mut local {
            group.claims = value["claims"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|claim| {
                    claim["volatile"] == false
                        && group
                            .names
                            .contains(&normalize(claim["subject"].as_str().unwrap_or_default()))
                })
                .filter_map(|claim| cited(claim.clone(), source, id, &date))
                .collect();
        }
        for p in procedures {
            let is_runbook = complete(&p);
            let title = normalize(p["title"].as_str().unwrap_or_default());
            // The digest shape has no procedure subject. Prefer a matching topic,
            // otherwise its first topic in normalized order. Complete procedures stand alone.
            let index = local
                .iter()
                .position(|g| g.template == "runbook" && is_runbook && g.names.contains(&title))
                .or_else(|| {
                    (!is_runbook)
                        .then(|| {
                            local
                                .iter()
                                .position(|g| {
                                    g.template != "runbook"
                                        && g.names.iter().any(|n| overlaps(n, &title))
                                })
                                .or_else(|| local.iter().position(|g| g.template == "topic"))
                                .or_else(|| local.iter().position(|g| g.template != "runbook"))
                        })
                        .flatten()
                });
            if let Some(index) = index {
                local[index].procedures.push(p);
            }
        }
        // Terms have no subject field either: prefer a glossary, otherwise the first topic.
        if let Some(index) = local
            .iter()
            .position(|g| g.template == "glossary")
            .or_else(|| local.iter().position(|g| g.template == "topic"))
        {
            local[index].terms = value["terms"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|term| cited(term.clone(), source, id, &date))
                .collect();
        }
        candidates.extend(local);
    }
    if batch.plan.sources.iter().any(|id| !found.contains(id)) {
        return Err(refusal(
            "digests_incomplete",
            "a confirmed source or digest is missing",
        ));
    }
    candidates.sort_by_key(|g| (g.key.clone(), g.template.clone()));
    // Include all matching page aliases before finding components. This allows a page
    // to bridge two source groups even when neither source named the other's alias.
    for group in &mut candidates {
        let singleton = template(templates, &group.template)?.one_per_library;
        let correct_kind = |page: &Page| {
            (page.template.as_deref() == Some("runbook")
                || page.kind == crate::domain::knowledge::DocumentKind::Runbook)
                == (group.template == "runbook")
        };
        let exact = |page: &Page| {
            correct_kind(page)
                && (!group.names.is_disjoint(&page_names(page))
                    || (singleton && page.template.as_deref() == Some(&group.template)))
        };
        let matched = pages.iter().find(|(page, _)| exact(page)).or_else(|| {
            pages.iter().find(|(page, _)| {
                correct_kind(page)
                    && group
                        .names
                        .iter()
                        .any(|name| page_names(page).iter().any(|other| overlaps(name, other)))
            })
        });
        if let Some((page, markdown)) = matched {
            group.names.extend(page_names(page));
            group.private |= page.sensitivity == Sensitivity::Private;
            group.template = page.template.clone().unwrap_or(group.template.clone());
            group.existing = Some((page.clone(), markdown.clone()));
        }
    }
    let mut groups = Vec::<Group>::new();
    for mut candidate in candidates {
        let mut index = 0;
        while index < groups.len() {
            let other = &groups[index];
            let same_singleton = candidate.template == other.template
                && template(templates, &candidate.template)?.one_per_library;
            let same_page = candidate
                .existing
                .as_ref()
                .zip(other.existing.as_ref())
                .is_some_and(|(a, b)| a.0.id == b.0.id);
            if same_page
                || same_singleton
                || ((candidate.template == "runbook") == (other.template == "runbook")
                    && !candidate.names.is_disjoint(&other.names))
            {
                let other = groups.remove(index);
                if candidate.existing.is_some() && other.existing.is_some() && !same_page {
                    return Err(refusal(
                        "ambiguous_subject",
                        "one subject matches multiple existing pages; resolve their aliases first",
                    ));
                }
                if (other.title.to_lowercase(), &other.title)
                    < (candidate.title.to_lowercase(), &candidate.title)
                {
                    candidate.title = other.title;
                }
                candidate.names.extend(other.names);
                candidate.sources.extend(other.sources);
                candidate.claims.extend(other.claims);
                candidate.procedures.extend(other.procedures);
                candidate.terms.extend(other.terms);
                candidate.private |= other.private;
                candidate.existing = candidate.existing.or(other.existing);
                if let Some((page, _)) = &candidate.existing {
                    candidate.template = page.template.clone().unwrap_or(candidate.template);
                }
                index = 0;
            } else {
                index += 1;
            }
        }
        groups.push(candidate);
    }
    groups.retain(|g| g.sources.iter().any(|id| batch.plan.sources.contains(id)));
    for group in &mut groups {
        group.key = group.existing.as_ref().map_or_else(
            || group.names.first().cloned().unwrap_or_default(),
            |(p, _)| p.id.clone(),
        );
        for items in [&mut group.claims, &mut group.procedures, &mut group.terms] {
            items.sort_by_key(Value::to_string);
            items.dedup();
        }
    }
    if !batch.plan.deferred_subjects.is_empty() {
        groups.retain(|group| {
            batch
                .plan
                .deferred_subjects
                .iter()
                .any(|subject| group.key == *subject || group.names.contains(subject))
                || batch
                    .plan
                    .groups
                    .iter()
                    .filter_map(|g| g.source.as_ref())
                    .any(|id| group.sources.contains(id))
        });
    }
    groups.sort_by_key(|g| g.key.clone());
    Ok(groups)
}
