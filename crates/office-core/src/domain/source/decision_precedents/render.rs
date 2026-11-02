//! Bounded evidence records, without scores or recommendations.
use super::contract::{STRING_BYTES, id_valid};
use super::{Candidate, Request};
use crate::domain::source::decisions::{DecidedBy, Decision, Options};
use serde_json::{Value, json};

pub(super) fn lexical_text(d: &Decision) -> String {
    let mut parts = vec![
        d.question.as_deref().unwrap_or_default(),
        d.chosen.as_deref().unwrap_or_default(),
    ];
    if let Some(Options::Listed(options)) = &d.options {
        parts.extend(options.iter().map(|o| o.text.as_str()));
    }
    if let Some(because) = &d.because {
        parts.push(&because.text);
    }
    parts.join(" ")
}

fn cut(text: &str, truncated: &mut bool) -> String {
    let mut end = text.len().min(STRING_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    *truncated |= end < text.len();
    // A long whitespace prefix must not turn a nonempty contract string
    // into an empty question/label after cutting it.
    if end < text.len() && text[..end].trim().is_empty() {
        return cut(text.trim_start(), truncated);
    }
    text[..end].to_owned()
}

fn fold(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fallback(
    chosen: &str,
    note: String,
    truncated: &mut bool,
) -> (Vec<Value>, String, Option<String>) {
    (
        vec![json!({"id":"chosen", "label":cut(chosen,truncated)})],
        "chosen".into(),
        Some(note),
    )
}

fn options(d: &Decision, truncated: &mut bool) -> (Vec<Value>, String, Option<String>) {
    let chosen = d.chosen.as_deref().unwrap_or_default();
    let Some(Options::Listed(list)) = &d.options else {
        return fallback(
            chosen,
            "Options not recorded; only the chosen option is known.".to_owned(),
            truncated,
        );
    };
    let exact: Vec<usize> = list
        .iter()
        .enumerate()
        .filter_map(|(i, o)| (fold(&o.text) == fold(chosen)).then_some(i))
        .collect();
    let matches: Vec<usize> = if exact.is_empty() {
        list.iter()
            .enumerate()
            .filter_map(|(i, o)| {
                crate::domain::source::decisions::chosen_matches(&o.text, chosen).then_some(i)
            })
            .collect()
    } else {
        exact
    };
    let mut ids = std::collections::BTreeSet::new();
    let wire_ids: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, o)| o.id.clone().unwrap_or_else(|| format!("o{}", i + 1)))
        .collect();
    if matches.len() != 1
        || list.len() > 8
        || wire_ids
            .iter()
            .any(|id| !id_valid(id, 64) || !ids.insert(id))
    {
        if list.len() > 8 {
            *truncated = true;
        }
        return fallback(
            chosen,
            format!(
                "Options not recorded; only the chosen option is known.\nRecovered options: {}",
                list.iter()
                    .map(|o| o.text.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            truncated,
        );
    }
    let options = list
        .iter()
        .zip(&wire_ids)
        .map(|(o, id)| json!({"id":id,"label":cut(&o.text,truncated)}))
        .collect();
    (options, wire_ids[matches[0]].clone(), None)
}

pub(super) fn precedent(
    request: &Request,
    row: &Candidate,
    partners: &[String],
    truncated: &mut bool,
) -> Value {
    let d = &row.decision;
    let (options, chosen, note) = options(d, truncated);
    let mut context = Vec::new();
    if let Some(note) = note {
        context.push(note);
    }
    if let Some(because) = &d.because {
        context.push(because.text.clone());
    }
    for partner in partners {
        context.push(format!(
            "Unresolved conflict with {partner}; the user has not said which stands."
        ));
    }
    if let Some(task_type) = &d.context.task_type {
        context.push(format!("task_type: {task_type}"));
    }
    if let Some(risk_tier) = &d.context.risk_tier {
        context.push(format!("risk_tier: {risk_tier}"));
    }
    // Scope binds retrieval to the querying workspace/category because
    // Orbit's v1 validator requires both strings, even for global entries.
    let category = d
        .orbit
        .as_ref()
        .and_then(|o| o.category.as_deref())
        .filter(|c| id_valid(c, 256))
        .unwrap_or(&request.request.category);
    let topic = d.orbit.as_ref().and_then(|o| o.topic.as_deref());
    let mut scope = json!({"workspace_id":request.workspace_id,"category":category});
    if let Some(topic) = topic {
        scope["topic"] = json!(cut(topic, truncated));
    }
    let kind = match d.decided_by {
        DecidedBy::Human => "human",
        DecidedBy::Model => "human_confirmed",
        DecidedBy::Agent => "human",
    };
    json!({"id":d.id,"question":cut(d.question.as_deref().unwrap_or_default(),truncated),"options":options,
        "chosen_option":chosen,"context":cut(&context.join("\n"),truncated),"scope":scope,
        "decided_at":d.decided_at,"decided_by":"user","decided_by_kind":kind,"reversed":!row.reversed_by.is_empty(),
        "citation":{"source":"office-decision-ledger","ref":format!("{} {}#L{}",d.id,row.ledger,row.line),"digest":row.digest}})
}
