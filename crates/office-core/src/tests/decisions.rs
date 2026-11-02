//! Decision ledgers: ids, line parsing, derived `reversed_by`, aliases and
//! the note migration's entry (specs/scribe.md, Decision ledger).

use crate::domain::knowledge::{DocumentKind, parse_citation};
use crate::domain::source::decisions::{
    EvidenceRef, Ledger, Resolved, Scope, decision_from_input, is_ledger_path, note_entry,
    note_source, parse_entry_id, resolve, views,
};
use serde_json::json;
use std::path::Path;

fn input(reverses: Option<&str>) -> serde_json::Value {
    json!({
        "question": "Which train?", "question_by": "scribe", "options": "unrecoverable",
        "chosen": "the March train", "quote": "#decision the March train", "ref": "src-x#L1",
        "scope": "project:acme", "decided_by": "human", "decided_at": "2026-11-01T10:00:00Z",
        "reverses": reverses
    })
}

fn ledger_with_two_entries() -> Ledger {
    let mut ledger = Ledger::new(
        Scope::parse("project:acme").unwrap(),
        "2026-11-01T10:00:00Z",
    );
    let mut text = ledger.render().unwrap();
    for reverses in [None, Some("dec-acme-1")] {
        let (mut decision, _) = decision_from_input(&input(reverses)).unwrap();
        decision.id = ledger.next_id();
        decision.source = "src-x".into();
        decision.validate().unwrap();
        text = crate::domain::source::decisions::append_line(&text, &decision.line().unwrap());
        ledger = Ledger::parse("sources/decisions/acme.md", &text).unwrap();
    }
    ledger
}

#[test]
fn scopes_ids_and_paths_name_one_ledger_file() {
    let scope = Scope::parse("project:orbit-office").unwrap();
    assert_eq!(scope.ledger_id(), "decisions-orbit-office");
    assert_eq!(scope.path(), "sources/decisions/orbit-office.md");
    assert_eq!(scope.entry_id(7), "dec-orbit-office-7");
    assert_eq!(
        Scope::parse("global").unwrap().path(),
        "sources/decisions/global.md"
    );
    assert!(Scope::parse("project:Bad Name").is_err());
    assert_eq!(
        parse_entry_id("dec-orbit-office-7"),
        Some(("orbit-office", 7))
    );
    for bad in ["dec-acme-0", "dec-acme-07", "dec-acme", "src-acme-1"] {
        assert_eq!(parse_entry_id(bad), None, "{bad}");
    }
    assert!(is_ledger_path(Path::new("sources/decisions/acme.md")));
    for other in [
        "sources/decisions/2026/acme.md",
        "sources/decisions/acme.txt",
        "sources/doc/acme.md",
        "../sources/decisions/acme.md",
    ] {
        assert!(!is_ledger_path(Path::new(other)), "{other}");
    }
}

#[test]
fn dec_and_ledger_ids_cite_as_sources() {
    for (text, id, anchor) in [
        ("dec-acme-1", "dec-acme-1", None),
        ("dec-acme-1#L1", "dec-acme-1", Some("L1")),
        ("decisions-acme#L3", "decisions-acme", Some("L3")),
    ] {
        let citation = parse_citation(text).unwrap();
        assert_eq!(citation.kind, DocumentKind::Source, "{text}");
        assert_eq!(citation.id, id);
        assert_eq!(citation.anchor.as_deref(), anchor);
    }
}

#[test]
fn reads_derive_reversed_by_and_written_lines_never_carry_it() {
    let ledger = ledger_with_two_entries();
    assert_eq!(ledger.entries.len(), 2);
    assert!(!ledger.render().unwrap().contains("reversed_by"));
    let all = views(std::slice::from_ref(&ledger));
    assert_eq!(all[0].reversed_by.as_deref(), Some("dec-acme-2"));
    assert_eq!(all[1].reversed_by, None);
    assert_eq!(all[0].ledger, "decisions-acme");
}

#[test]
fn a_ledger_out_of_sequence_or_with_unknown_fields_is_refused() {
    let ledger = ledger_with_two_entries();
    let text = ledger.render().unwrap();
    let swapped = text.replace("dec-acme-2", "dec-acme-3");
    let error = Ledger::parse("sources/decisions/acme.md", &swapped).unwrap_err();
    assert_eq!(error.code(), "decision_ledger_invalid");
    let wrong_path = Ledger::parse("sources/decisions/other.md", &text).unwrap_err();
    assert_eq!(wrong_path.code(), "decision_ledger_invalid");
    let mut extra = input(None);
    extra["reversed_by"] = json!("dec-acme-2");
    assert_eq!(
        decision_from_input(&extra).unwrap_err().code(),
        "invalid_decision"
    );
}

#[test]
fn precedent_fields_round_trip_and_existing_entries_still_read() {
    let mut value = input(None);
    value["same_as"] = json!("dec-acme-1");
    let (mut decision, _) = decision_from_input(&value).unwrap();
    // decision_from_input only sets same_as; ingestion sets the rest directly.
    decision.options = Some(crate::domain::source::decisions::Options::Listed(vec![
        crate::domain::source::decisions::Cited {
            text: "the March train".into(),
            reference: "src-x#L1".into(),
            id: Some("a1".into()),
            consequence: Some("Ships in March.".into()),
        },
    ]));
    decision.id = "dec-acme-2".into();
    decision.source = "orbit-decision:REQ-1".into();
    decision.orbit = Some(crate::domain::source::decisions::OrbitInfo {
        request_id: Some("REQ-1".into()),
        gate: Some("ship".into()),
        category: Some("release".into()),
        topic: None,
        answered_by: Some("model".into()),
        channel: Some("task".into()),
        model: Some(crate::domain::source::decisions::OrbitModel {
            assessment_id: Some("asm-1".into()),
            policy_digest: Some("sha256:abc".into()),
            model: Some("model-1".into()),
            probability: Some("0.8".into()),
            cited_precedents: Some(vec!["dec-acme-1".into()]),
        }),
    });
    decision.confirms = Some("dec-acme-1".into());
    decision.validate().unwrap();
    let line = decision.line().unwrap();
    assert!(line.contains("\"id\":\"a1\""), "{line}");
    assert!(line.contains("\"same_as\":\"dec-acme-1\""), "{line}");
    assert!(line.contains("\"confirms\":\"dec-acme-1\""), "{line}");
    let parsed: crate::domain::source::decisions::Decision =
        serde_json::from_str(line.split(" #decision ").nth(1).unwrap()).unwrap();
    assert_eq!(parsed, decision);
    // The same line reads back as the second entry of its ledger.
    let first = {
        let (mut d, _) = decision_from_input(&input(None)).unwrap();
        d.id = "dec-acme-1".into();
        d.source = "src-x".into();
        d.line().unwrap()
    };
    let text = crate::domain::source::decisions::append_line(
        &crate::domain::source::decisions::append_line(
            &Ledger::new(
                Scope::parse("project:acme").unwrap(),
                "2026-11-01T10:00:00Z",
            )
            .render()
            .unwrap(),
            &first,
        ),
        &line,
    );
    let ledger = Ledger::parse("sources/decisions/acme.md", &text).unwrap();
    assert_eq!(ledger.entries[1].decision, decision);
    // An entry without the new fields serializes without them.
    let (plain, _) = decision_from_input(&input(None)).unwrap();
    let plain_line = serde_json::to_string(&plain).unwrap();
    assert!(!plain_line.contains("same_as"), "{plain_line}");
    assert!(!plain_line.contains("orbit"), "{plain_line}");
}

#[test]
fn capture_input_refuses_ingestion_only_fields_and_accepts_same_as() {
    for extra in [
        json!({"orbit": {"request_id": "REQ-1"}}),
        json!({"confirms": "dec-acme-1"}),
    ] {
        let mut value = input(None);
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(
            decision_from_input(&value).unwrap_err().code(),
            "invalid_decision",
            "{extra}"
        );
    }
    let mut with_id = input(None);
    with_id["options"] = json!([{"text": "the March train", "ref": "src-x#L1", "id": "a1"}]);
    assert_eq!(
        decision_from_input(&with_id).unwrap_err().code(),
        "invalid_decision"
    );
    let mut with_consequence = input(None);
    with_consequence["options"] =
        json!([{"text": "the March train", "ref": "src-x#L1", "consequence": "Ships."}]);
    assert_eq!(
        decision_from_input(&with_consequence).unwrap_err().code(),
        "invalid_decision"
    );
    let mut with_same_as = input(None);
    with_same_as["same_as"] = json!("dec-acme-1");
    let (decision, _) = decision_from_input(&with_same_as).unwrap();
    assert_eq!(decision.same_as.as_deref(), Some("dec-acme-1"));
    assert!(decision.orbit.is_none() && decision.confirms.is_none());
}

#[test]
fn same_as_match_links_exactly_one_ingested_entry() {
    use crate::domain::source::decisions::{OrbitInfo, same_as_match};
    let ingested = |id: &str, at: &str, chosen: &str, task: Option<&str>| {
        let (mut d, _) = decision_from_input(&json!({
            "question": "Which train?", "question_by": "scribe", "options": "unrecoverable",
            "chosen": chosen, "quote": format!("#decision {chosen}"), "ref": "src-x#L1",
            "scope": "project:acme", "decided_by": "human", "decided_at": at,
            "context": {"task_type": null, "workspace": null, "task": task, "risk_tier": null}
        }))
        .unwrap();
        d.id = id.into();
        d.source = "orbit-decision:REQ-1".into();
        d.orbit = Some(OrbitInfo {
            request_id: Some("REQ-1".into()),
            gate: None,
            category: None,
            topic: None,
            answered_by: Some("human".into()),
            channel: None,
            model: None,
        });
        d
    };
    let candidate = |at: &str| {
        let (mut d, _) = decision_from_input(&json!({
            "question": "Which train?", "question_by": "scribe", "options": "unrecoverable",
            "chosen": "the March train", "quote": "#decision the March train", "ref": "src-x#L1",
            "scope": "project:acme", "decided_by": "human", "decided_at": at,
            "context": {"task_type": null, "workspace": null, "task": "ORB-1", "risk_tier": null}
        }))
        .unwrap();
        d.id = "dec-acme-9".into();
        d
    };
    let one = ingested(
        "dec-acme-1",
        "2026-11-01T10:10:00Z",
        "The March  Train",
        Some("ORB-1"),
    );
    assert_eq!(
        same_as_match(&candidate("2026-11-01T10:00:00Z"), [&one]),
        Some("dec-acme-1".into())
    );
    // Ambiguous: two matches link nothing.
    let two = ingested(
        "dec-acme-2",
        "2026-11-01T10:05:00Z",
        "the March train",
        Some("ORB-1"),
    );
    assert_eq!(
        same_as_match(&candidate("2026-11-01T10:00:00Z"), [&one, &two]),
        None
    );
    // Outside 30 minutes, another task, or a different choice: no match.
    assert_eq!(
        same_as_match(&candidate("2026-11-01T11:00:00Z"), [&one]),
        None
    );
    let other_task = ingested(
        "dec-acme-3",
        "2026-11-01T10:10:00Z",
        "the March train",
        Some("ORB-2"),
    );
    assert_eq!(
        same_as_match(&candidate("2026-11-01T10:00:00Z"), [&other_task]),
        None
    );
}

#[test]
fn chosen_must_be_a_span_of_the_quote() {
    let mut value = input(None);
    value["chosen"] = json!("the June train");
    let (decision, _) = decision_from_input(&value).unwrap();
    assert_eq!(decision.validate().unwrap_err().code(), "quote_mismatch");
}

#[test]
fn evidence_refs_name_a_source_anchor_or_a_task_comment() {
    assert_eq!(
        EvidenceRef::parse("orbit://task/TASK-1#comment-2").unwrap(),
        EvidenceRef::TaskComment {
            task: "TASK-1".into(),
            number: 2
        }
    );
    assert_eq!(EvidenceRef::parse("src-x#L4").unwrap().source_id(), "src-x");
    for bad in [
        "src-x",
        "orbit://task/TASK-1#comment-0",
        "https://example.invalid",
    ] {
        assert_eq!(
            EvidenceRef::parse(bad).unwrap_err().code(),
            "decision_ref_unresolved",
            "{bad}"
        );
    }
}

#[test]
fn a_migrated_note_resolves_by_its_old_id_with_its_body_as_the_quote() {
    let markdown = "---\nid: src-20261101-direction\nkind: note\ntitle: Retire notes\ntags: [project:acme]\noccurred_at: 2026-11-01T09:00:00Z\n---\nThere are no users.\nDeprecate outright.\n";
    let note = note_source(
        "sources/note/2026/11/src-20261101-direction/source.md",
        markdown,
        "2026-11-02T00:00:00Z",
    )
    .unwrap();
    assert_eq!(note.scope, Scope::parse("project:acme").unwrap());
    assert_eq!(note.decided_at, "2026-11-01T09:00:00Z");
    assert!(
        note_source(
            "x/source.md",
            &markdown.replace("kind: note", "kind: doc"),
            "t"
        )
        .is_none()
    );

    let mut ledger = Ledger::new(note.scope.clone(), "2026-11-02T00:00:00Z");
    let entry = note_entry(
        &note.scope,
        &ledger.next_id(),
        &note.id,
        &note.title,
        &note.body,
        &note.decided_at,
    )
    .unwrap();
    assert_eq!(entry.reference, "src-20261101-direction#L1-2");
    assert_eq!(entry.chosen.as_deref(), Some("There are no users."));
    ledger
        .moved_sources
        .insert(note.id.clone(), entry.id.clone());
    let text = crate::domain::source::decisions::append_line(
        &ledger.render().unwrap(),
        &entry.line().unwrap(),
    );
    let ledger = Ledger::parse("sources/decisions/acme.md", &text).unwrap();
    let ledgers = [ledger];
    match resolve(&ledgers, "src-20261101-direction") {
        Some(Resolved::Entry {
            entry, moved_from, ..
        }) => {
            assert_eq!(entry.decision.id, "dec-acme-1");
            assert_eq!(moved_from.as_deref(), Some("src-20261101-direction"));
        }
        Some(Resolved::Ledger(_)) | None => panic!("the old id resolves to its entry"),
    }
}
