//! Public apply API: provenance, size gates, conflicts and write validation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::domain::knowledge::preferences::{Conflict, Preference};
use office_core::ports::Library;
use office_testkit::fakes::FakeLibrary;
use office_tools::preferences::{ApplyOutcome, Provenance, apply_preference};
use std::path::Path;

fn fixture() -> FakeLibrary {
    let library = FakeLibrary::with_bundled_templates();
    library.write_document(Path::new("sources/conversation/2026/11/SCR-20261101-2231-0a3104.md"),
        "---\nid: SCR-20261101-2231-0a3104\nkind: conversation\n---\n## Tags\n### Preferences\n- T1 #preferences concise replies (m-1)\n").unwrap();
    library
}
fn preference(matter: &str, statement: &str) -> Preference {
    Preference::new(
        matter.into(),
        statement.into(),
        vec!["SCR-20261101-2231-0a3104#T1".into()],
        "2026-11-01".into(),
        None,
    )
    .unwrap()
}
fn page(preferences: &[Preference]) -> String {
    format!(
        "---\nid: dom-preferences-global\ntitle: Global preferences\ntemplate: preferences\n---\n{}",
        preferences
            .iter()
            .map(Preference::markdown)
            .collect::<String>()
    )
}

#[test]
fn apply_writes_user_markers_and_defaults_to_ninety_days() {
    let library = fixture();
    let record = preference("replies", "Prefer concise replies.");
    assert_eq!(record.review_by, "2027-01-30");
    let markdown = page(&[record]);
    assert_eq!(
        apply_preference(
            &library,
            &markdown,
            Provenance::UserMarker,
            "#preferences concise replies",
            &[]
        )
        .unwrap(),
        ApplyOutcome::Applied
    );
    let path = Path::new("knowledge/domain/dom-preferences-global.md");
    assert_eq!(library.read_text(path).unwrap(), markdown);
    assert_eq!(library.history(path).unwrap().len(), 1);
    // An unchanged statement with newer metadata can be written, and a new
    // matter extends the page. Neither is a contradiction.
    let updated = page(&[
        preference("replies", "Prefer concise replies."),
        preference("tests", "Run focused tests."),
    ]);
    assert_eq!(
        apply_preference(
            &library,
            &updated,
            Provenance::UserMarker,
            "#preferences run focused tests",
            &[]
        )
        .unwrap(),
        ApplyOutcome::Applied
    );
    assert_eq!(library.read_text(path).unwrap(), updated);
}

#[test]
fn apply_returns_propose_without_any_write_for_every_excluded_candidate() {
    for (provenance, turn, semantic, expected) in [
        (
            Provenance::AgentMarker,
            "#preferences concise".into(),
            vec![],
            "untrusted_candidate",
        ),
        (
            Provenance::Inferred,
            "concise".into(),
            vec![],
            "untrusted_candidate",
        ),
        (
            Provenance::Untrusted,
            "#preferences concise".into(),
            vec![],
            "untrusted_candidate",
        ),
        (
            Provenance::UserMarker,
            "a\n".repeat(41),
            vec![],
            "paste_sized_turn",
        ),
        (
            Provenance::UserMarker,
            "é".repeat(4001),
            vec![],
            "paste_sized_turn",
        ),
        (
            Provenance::UserMarker,
            "#preferences concise".into(),
            vec![Conflict {
                matter: "style".into(),
                statement: "Detailed.".into(),
                cites: vec!["dec-global-1".into()],
            }],
            "conflict",
        ),
    ] {
        let library = fixture();
        let before = library.head_commit().unwrap();
        assert_eq!(
            apply_preference(
                &library,
                &page(&[preference("replies", "Concise.")]),
                provenance,
                &turn,
                &semantic
            )
            .unwrap(),
            ApplyOutcome::Propose(expected)
        );
        assert_eq!(library.head_commit().unwrap(), before);
        assert!(library.get("dom-preferences-global").unwrap().is_none());
    }
}

#[test]
fn apply_size_boundaries_are_inclusive_and_count_characters() {
    for turn in ["é".repeat(4000), "a\n".repeat(40)] {
        let library = fixture();
        assert_eq!(
            apply_preference(
                &library,
                &page(&[preference("replies", "Concise.")]),
                Provenance::UserMarker,
                &turn,
                &[]
            )
            .unwrap(),
            ApplyOutcome::Applied
        );
    }
}

#[test]
fn apply_detects_current_statement_conflicts_and_refuses_invalid_pages() {
    let library = fixture();
    let path = Path::new("knowledge/domain/dom-preferences-global.md");
    let old = page(&[preference("replies", "Detailed.")]);
    library.write_document(path, &old).unwrap();
    let head = library.head_commit().unwrap();
    assert_eq!(
        apply_preference(
            &library,
            &page(&[preference("replies", "Concise.")]),
            Provenance::UserMarker,
            "#preferences concise",
            &[]
        )
        .unwrap(),
        ApplyOutcome::Propose("conflict")
    );
    assert_eq!(library.read_text(path).unwrap(), old);
    for invalid in [
        old.replace("#T1", "#T999"),
        old.replace("template: preferences", "template: absent"),
        old.replace("review_by: 2027-01-30\n", ""),
        "---\nid: dom-other\ntitle: Other\n---\nBody".into(),
    ] {
        assert!(
            apply_preference(
                &library,
                &invalid,
                Provenance::UserMarker,
                "#preferences concise",
                &[]
            )
            .is_err()
        );
        assert_eq!(library.head_commit().unwrap(), head);
    }
}

#[test]
fn apply_checks_sharing_approval_for_quoted_matter_citations() {
    let library = fixture();
    let markdown = page(&[preference("replies", "Concise.")]).replace(
        "template: preferences",
        "template: preferences\nshareable: true",
    );
    let before = library.head_commit().unwrap();
    let error = apply_preference(
        &library,
        &markdown,
        Provenance::UserMarker,
        "#preferences concise",
        &[],
    )
    .unwrap_err();
    assert_eq!(error.code(), "share_cites_unshared");
    assert_eq!(library.head_commit().unwrap(), before);
}

#[test]
fn apply_detects_a_concurrent_page_creation_without_overwriting_it() {
    use office_core::{Result, domain::knowledge::DocumentRecord, ports::LibraryText};
    struct Concurrent {
        library: FakeLibrary,
        other: String,
    }
    impl Library for Concurrent {
        fn templates(&self) -> Result<Vec<LibraryText>> {
            self.library.templates()
        }
        fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
            self.library.knowledge_pages()
        }
        fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
            self.library.get(id)
        }
        fn write_reviewed_page(
            &self,
            path: &Path,
            text: &str,
            expected: Option<&str>,
        ) -> Result<DocumentRecord> {
            self.library.write_document(path, &self.other)?;
            self.library.write_reviewed_page(path, text, expected)
        }
    }
    let concurrent = Concurrent {
        library: fixture(),
        other: page(&[preference("replies", "Detailed.")]),
    };
    assert_eq!(
        apply_preference(
            &concurrent,
            &page(&[preference("replies", "Concise.")]),
            Provenance::UserMarker,
            "#preferences concise",
            &[]
        )
        .unwrap(),
        ApplyOutcome::Propose("revision_changed")
    );
    assert_eq!(
        concurrent
            .library
            .read_text(Path::new("knowledge/domain/dom-preferences-global.md"))
            .unwrap(),
        concurrent.other
    );
}
