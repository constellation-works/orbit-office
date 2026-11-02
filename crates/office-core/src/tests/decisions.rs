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
