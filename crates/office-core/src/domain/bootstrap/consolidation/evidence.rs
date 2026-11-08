//! Shared evidence helpers: cited items, procedures and candidate subjects.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::super::{Source, normalize};
use crate::domain::knowledge::Page;
use crate::domain::metrics::timestamp;
use crate::domain::source::{anchor_span, parse_anchor, source_body};
use crate::domain::template::Template;
use crate::{Error, Result};

pub(super) fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
pub(super) fn names(subject: &Value) -> BTreeSet<String> {
    let mut names = strings(&subject["aliases"])
        .into_iter()
        .map(|s| normalize(&s))
        .collect::<BTreeSet<_>>();
    names.insert(normalize(subject["name"].as_str().unwrap_or_default()));
    names.remove("");
    names
}

/// Jaccard overlap of distinct normalized title tokens, using integers at the boundary.
#[must_use]
pub(super) fn overlaps(left: &str, right: &str) -> bool {
    let left = left.split_whitespace().collect::<BTreeSet<_>>();
    let right = right.split_whitespace().collect::<BTreeSet<_>>();
    !left.is_empty() && left.intersection(&right).count() * 5 >= left.union(&right).count() * 3
}

pub(super) fn page_names(page: &Page) -> BTreeSet<String> {
    let mut names = page
        .aliases
        .iter()
        .map(|s| normalize(s))
        .collect::<BTreeSet<_>>();
    names.insert(normalize(&page.title));
    names.remove("");
    names
}
pub(super) fn template<'a>(
    templates: &'a BTreeMap<String, Template>,
    name: &str,
) -> Result<&'a Template> {
    templates.get(name).ok_or_else(|| Error::UnknownTemplate {
        id: "bootstrap".into(),
        template: name.into(),
    })
}
pub(super) fn cited(mut item: Value, source: &Source, id: &str, date: &Value) -> Option<Value> {
    let cites = strings(&item["cites"]);
    let body = source_body(&source.markdown);
    if cites.is_empty()
        || !cites.iter().all(|cite| {
            cite.starts_with('#')
                && parse_anchor(cite)
                    .and_then(|anchor| anchor_span(&body, anchor, cite))
                    .is_ok()
        })
    {
        return None;
    }
    item["cites"] = json!(
        cites
            .iter()
            .map(|cite| format!("{id}{cite}"))
            .collect::<Vec<_>>()
    );
    item["source"] = json!(id);
    item["as_of"] = date.clone();
    Some(item)
}
pub(in crate::domain::bootstrap) fn procedure(
    raw: &Value,
    source: &Source,
    id: &str,
    date: &Value,
) -> Value {
    let steps = raw["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| cited(step.clone(), source, id, date))
        .collect::<Vec<_>>();
    json!({"title":raw["title"],"inputs":strings(&raw["inputs"]),"steps":steps,"done_when":raw["done_when"],"source":id,"as_of":date})
}
pub(super) fn complete(procedure: &Value) -> bool {
    procedure["inputs"].as_array().is_some_and(|v| {
        !v.is_empty()
            && v.iter()
                .all(|s| s.as_str().is_some_and(|s| !s.trim().is_empty()))
    }) && procedure["steps"].as_array().is_some_and(|v| v.len() >= 2)
        && procedure["done_when"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty())
}

/// Candidate subjects share the runbook-shape rule with allowance previews.
pub(in crate::domain::bootstrap) fn candidate_subjects(
    value: &Value,
    procedures: &[Value],
) -> Vec<Value> {
    let mut subjects = value["subjects"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let has_topic = subjects.iter().any(|s| s["template"] == "topic");
    subjects.retain_mut(|subject| {
        if subject["template"] != "runbook" {
            return true;
        }
        let names = names(subject);
        if procedures.iter().any(|p| {
            complete(p) && names.contains(&normalize(p["title"].as_str().unwrap_or_default()))
        }) {
            return true;
        }
        if has_topic {
            return false;
        }
        subject["template"] = json!("topic");
        true
    });
    for procedure in procedures {
        if complete(procedure) {
            subjects.push(json!({"name":procedure["title"],"template":"runbook"}));
        } else if !subjects.iter().any(|s| s["template"] == "topic") {
            subjects.push(json!({"name":procedure["title"],"template":"topic"}));
        }
    }
    subjects
}

pub(super) fn date_seconds(value: &Value) -> Option<i64> {
    let date = value.as_str()?;
    timestamp(&if date.len() == 10 {
        format!("{date}T00:00:00Z")
    } else {
        date.into()
    })
}
