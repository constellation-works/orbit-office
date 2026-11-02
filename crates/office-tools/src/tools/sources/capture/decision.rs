//! `office.capture` with a `decision` input: append one decision ledger
//! entry (specs/scribe.md, Decision ledger). Every verbatim field is
//! checked against the source anchor or task comment its ref names before
//! the Library appends it.

use std::collections::BTreeMap;

use office_core::domain::source::decisions::{EvidenceRef, decision_from_input};
use office_core::domain::source::{anchor_span, format_rfc3339_utc, parse_anchor};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::Call;

pub(in crate::tools::sources) fn run(call: &Call<'_>) -> Result<Value> {
    if let Some(key) = call.input.keys().find(|key| key.as_str() != "decision") {
        return Err(Error::InvalidArgument(format!(
            "capture decision cannot be combined with {key}"
        )));
    }
    let raw = call
        .input
        .get("decision")
        .ok_or_else(|| Error::InvalidArgument("capture decision is required".into()))?;
    let (mut decision, claimed_source) = decision_from_input(raw)?;
    let main = EvidenceRef::parse(&decision.reference)?;
    let source = main.source_id();
    if claimed_source.is_some_and(|claimed| claimed != source) {
        return Err(Error::Library {
            code: "invalid_decision",
            message: format!("decision source must be {source}, the source its ref names"),
        });
    }
    decision.source = source;
    decision.validate()?;
    let mut cache: BTreeMap<String, String> = BTreeMap::new();
    let private = decision.private;
    decision.verify(|reference| {
        if let Some(text) = cache.get(reference) {
            return Ok(text.clone());
        }
        let text = evidence(call, reference, private)?;
        cache.insert(reference.to_owned(), text.clone());
        Ok(text)
    })?;
    let outcome = call
        .ports
        .library
        .append_decision(&decision, &format_rfc3339_utc(call.ports.clock.now()))?;
    let ledger = outcome.entry.ledger.clone();
    Ok(json!({
        "id": outcome.entry.decision.id,
        "kind": office_core::domain::source::decisions::KIND,
        "title": outcome.title,
        "path": outcome.path,
        "sha256": null,
        "bytes": null,
        "media_type": null,
        "sensitivity": "normal",
        "deduplicated": false,
        "appended": 1,
        "version": 1,
        "ledger": ledger,
        "entry": outcome.entry,
    }))
}

/// The text a ref names. A private source's text never reaches a ledger,
/// so citing one is refused unless the entry is private (reference-only).
fn evidence(call: &Call<'_>, reference: &str, private_entry: bool) -> Result<String> {
    let unresolved = |reason: &str| Error::Library {
        code: "decision_ref_unresolved",
        message: format!("decision ref {reference} cannot be read: {reason}"),
    };
    match EvidenceRef::parse(reference)? {
        EvidenceRef::Source { id, anchor } => {
            let view = call.ports.library.read_source(&id).map_err(|error| {
                if matches!(
                    error,
                    Error::Library {
                        code: "source_not_found",
                        ..
                    }
                ) {
                    unresolved("no source has this id")
                } else {
                    error
                }
            })?;
            if view.sensitivity == "private" && !private_entry {
                return Err(Error::Library {
                    code: "decision_source_private",
                    message: format!(
                        "decision ref {reference} names a private source; record a private (reference-only) entry instead"
                    ),
                });
            }
            let parsed = parse_anchor(&anchor)?;
            anchor_span(&view.body, parsed, &anchor)
        }
        EvidenceRef::TaskComment { task, number } => {
            let input = json!({"id": task});
            let input = input.as_object().ok_or(Error::OrbitUnavailable)?;
            let shown = call.ports.orbit.task_show(input)?;
            let record = shown.get("task").unwrap_or(&shown);
            if record["id"].as_str() != Some(task.as_str()) {
                return Err(unresolved("Orbit returned a different task"));
            }
            let comments = record["comments"]
                .as_array()
                .ok_or_else(|| unresolved("the task has no comments"))?;
            comments
                .get(number - 1)
                .and_then(|comment| comment["message"].as_str())
                .map(str::to_owned)
                .ok_or_else(|| unresolved("the task has no such comment"))
        }
    }
}
