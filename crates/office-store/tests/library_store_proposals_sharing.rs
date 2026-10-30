//! Proposal and sharing scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::LibraryStore;
use std::path::Path;

#[test]
fn proposal_records_commit_without_entering_the_document_index() {
    use office_core::ports::Library;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    let before = store.document_count().unwrap();
    let path = Path::new("proposals/proposal-fixture.md");
    Library::write_proposal(&store, path, "# Proposal evidence\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(path)).unwrap(),
        "# Proposal evidence\n"
    );
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(store.reindex().unwrap(), before);
    assert_eq!(store.document_count().unwrap(), before);
    for path in [
        "knowledge/domain/page.md",
        "proposals/../../escape.md",
        "proposals/nested/page.md",
    ] {
        assert!(Library::write_proposal(&store, Path::new(path), "refused").is_err());
    }
}

#[test]
fn reviewed_pages_compare_the_revision_under_the_writer_lock() {
    use office_core::ports::Library;
    let temp = tempfile::tempdir().unwrap();
    let store =
        LibraryStore::open(&temp.path().join("library"), &temp.path().join("state")).unwrap();
    Library::initialize(&store, None).unwrap();
    let path = Path::new("knowledge/domain/dom-review.md");
    let first = "---\nid: dom-review\ntitle: Review\n---\nOriginal\n";
    Library::write_reviewed_page(&store, path, first, None).unwrap();
    let hash = office_core::domain::source::sha256_hex(first.as_bytes());
    let changed = first.replace("Original", "Changed");
    Library::write_reviewed_page(&store, path, &changed, Some(&hash)).unwrap();
    let head = store.head_commit().unwrap();
    assert!(Library::write_reviewed_page(&store, path, first, Some(&hash)).is_err());
    assert!(Library::write_reviewed_page(&store, path, first, None).is_err());
    assert_eq!(store.read_text(path).unwrap(), changed);
    assert_eq!(store.head_commit().unwrap(), head);
}

#[test]
fn proposal_journal_replays_on_open_and_refuses_symlink_targets() {
    use office_core::ports::Library;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    drop(store);
    let pending = serde_json::json!({"version":8,"kind":"proposal","root":root.canonicalize().unwrap(),"relative":"proposals/recovered.md","text":"Recovered evidence","token":"123-456"});
    std::fs::write(
        state.join("library.pending-write.json"),
        pending.to_string(),
    )
    .unwrap();
    let store = LibraryStore::open(&root, &state).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("proposals/recovered.md")).unwrap(),
        "Recovered evidence"
    );
    assert!(!state.join("library.pending-write.json").exists());
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            temp.path().join("outside.md"),
            root.join("proposals/unsafe.md"),
        )
        .unwrap();
        assert!(
            Library::write_proposal(&store, Path::new("proposals/unsafe.md"), "refused").is_err()
        );
        assert!(!temp.path().join("outside.md").exists());
    }
}

#[test]
fn meeting_prep_duplicate_candidates_are_keyed_under_the_library_writer_lock() {
    use office_core::domain::artifact::Approver;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    let mut candidate = artifact_candidate();
    candidate.series = "meeting-prep".into();
    candidate.event_key = Some("weekly:2026-10-28T14:00:00Z".into());
    candidate.task = "routine:office-meeting-prep/weekly:2026-10-28T14:00:00Z".into();
    candidate.approved.by = Approver::Routine;
    candidate.approved.edited = false;
    // Deterministic race harness: two callers gathered this same candidate
    // before either publishes. The second must lose under the writer lock.
    let saved = store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap();
    let head = store.head_commit().unwrap();
    candidate.task = "different-producer-same-event".into();
    let error = store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap_err();
    assert_eq!(error.code(), "meeting_prep_already_exists");
    assert_eq!(store.head_commit().unwrap(), head);
    assert_eq!(store.artifact_documents().unwrap().len(), 1);
    let text = store.artifact_documents().unwrap()[0]
        .markdown
        .clone()
        .unwrap();
    assert_eq!(
        office_core::domain::artifact::Artifact::from_markdown(&text).unwrap(),
        saved
    );
    candidate.event_key = Some("weekly:2026-11-04T14:00:00Z".into());
    let next = store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap();
    assert_eq!(next.version, 2);
    assert_eq!(next.supersedes, Some(saved.id));
    // The exception never widens routine approval for other artifact series.
    candidate.series = "unapproved-series".into();
    assert_eq!(
        store
            .promote_artifact(&candidate, &artifact_files())
            .unwrap_err()
            .code(),
        "artifact_routine_approval_refused"
    );
    candidate.series = "meeting-prep".into();
    candidate.event_key = None;
    assert_eq!(
        store
            .promote_artifact(&candidate, &artifact_files())
            .unwrap_err()
            .code(),
        "artifact_routine_approval_refused"
    );
}

#[test]
fn sharing_updates_all_document_kinds_with_one_commit_and_no_other_byte_changes() {
    use office_core::ports::Library;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    for (id, path) in [
        ("src-shared", "sources/2026/10/src-shared/source.md"),
        ("dom-shared", "knowledge/domain/dom-shared.md"),
        (
            "art-20261028-shared",
            "artifacts/2026/10/art-20261028-shared/artifact.md",
        ),
    ] {
        let text = format!(
            "---\r\nid: {id}\r\ntitle: Keep  spacing # comment\r\nsensitivity: normal\r\n---\r\nBody  bytes.\r\n"
        );
        store.write_document(Path::new(path), &text).unwrap();
        let original_head = store.head_commit().unwrap();
        let shared = store.set_shareable(id, true).unwrap();
        assert!(shared.shareable);
        let first_head = store.head_commit().unwrap();
        assert_ne!(first_head, original_head);
        assert_eq!(store.history(Path::new(path)).unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(root.join(path)).unwrap(),
            text.replace("---\r\nBody", "shareable: true\r\n---\r\nBody")
        );
        assert!(store.get(id).unwrap().unwrap().shareable);
        store.set_shareable(id, true).unwrap();
        assert_eq!(store.head_commit().unwrap(), first_head);
        store.set_shareable(id, false).unwrap();
        let cleared_head = store.head_commit().unwrap();
        assert_ne!(cleared_head, first_head);
        assert_eq!(store.history(Path::new(path)).unwrap().len(), 3);
        assert_eq!(
            std::fs::read_to_string(root.join(path)).unwrap(),
            text.replace("---\r\nBody", "shareable: false\r\n---\r\nBody")
        );
        assert!(!store.get(id).unwrap().unwrap().shareable);
        store.set_shareable(id, false).unwrap();
        assert_eq!(store.head_commit().unwrap(), cleared_head);
    }
    let source = "sources/2026/10/src-shared/source.md";
    store.set_shareable("src-shared", true).unwrap();
    let count = store.history(Path::new(source)).unwrap().len();
    let outcome = Library::set_source_sensitivity(&store, "src-shared", "private").unwrap();
    assert!(outcome.unshared);
    assert_eq!(outcome.source.sensitivity, "private");
    assert!(!store.get("src-shared").unwrap().unwrap().shareable);
    assert_eq!(store.history(Path::new(source)).unwrap().len(), count + 1);
    let head = store.head_commit().unwrap();
    assert!(
        !Library::set_source_sensitivity(&store, "src-shared", "private")
            .unwrap()
            .unshared
    );
    assert_eq!(store.head_commit().unwrap(), head);
}

#[test]
fn sharing_refusals_do_not_write_or_commit_and_use_latest_cited_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    let source_path = Path::new("sources/2026/10/src-proof/source.md");
    let evidence = "---\nid: src-proof\ntitle: Proof\n---\nLine.\n";
    store.write_document(source_path, evidence).unwrap();
    let path = Path::new("knowledge/domain/dom-sharing.md");
    let page = "---\nid: dom-sharing\ntitle: Sharing\ncites: [src-proof#L1]\nrefs: [{label: Output, artifact: art-20261028-unshared}]\n---\nBody [src-inline#L1; src-proof#L1].\n";
    store.write_document(path, page).unwrap();
    let head = store.head_commit().unwrap();
    let error = store.set_shareable("dom-sharing", true).unwrap_err();
    assert_eq!(error.code(), "share_cites_unshared");
    assert_eq!(
        error.to_string(),
        "cites unshared own documents: art-20261028-unshared, src-inline, src-proof"
    );
    assert_eq!(store.head_commit().unwrap(), head);
    assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), page);
    assert!(!store.get("dom-sharing").unwrap().unwrap().shareable);
    assert_eq!(
        store.set_shareable("dom-missing", true).unwrap_err().code(),
        "document_not_found"
    );
    let private = "---\nid: dom-private\ntitle: Private\nsensitivity: private\n---\nBody.\n";
    store
        .write_document(Path::new("knowledge/domain/dom-private.md"), private)
        .unwrap();
    let head = store.head_commit().unwrap();
    let error = store.set_shareable("dom-private", true).unwrap_err();
    assert_eq!(error.code(), "share_private_refused");
    assert!(error.to_string().contains("sensitivity"));
    assert_eq!(store.head_commit().unwrap(), head);
    // The index says shared; an external edit revokes it before approval.
    store.set_shareable("src-proof", true).unwrap();
    std::fs::write(root.join(source_path), evidence).unwrap();
    let only_proof =
        "---\nid: dom-proof\ntitle: Proof\nshareable: true\ncites: [src-proof#L1]\n---\nBody.\n";
    let head = store.head_commit().unwrap();
    assert_eq!(
        store
            .write_reviewed_page(Path::new("knowledge/domain/dom-proof.md"), only_proof, None)
            .unwrap_err()
            .code(),
        "share_cites_unshared"
    );
    assert_eq!(store.head_commit().unwrap(), head);
    assert!(!root.join("knowledge/domain/dom-proof.md").exists());
}

#[test]
fn source_processing_preserves_sharing_approval_in_file_and_index() {
    use office_core::domain::{endpoint::ModelRoute, source::sha256_hex};
    use office_core::ports::{CaptureSourceRequest, DerivedBy, Library};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    let captured = store
        .capture_source(&CaptureSourceRequest {
            captured_at: "2026-10-28T00:00:00Z".into(),
            kind: Some("meeting".into()),
            title: Some("Shared meeting".into()),
            text: Some("Original transcript".into()),
            ..Default::default()
        })
        .unwrap();
    store.set_shareable(&captured.id, true).unwrap();
    let markdown = store.read_text(Path::new(&captured.path)).unwrap();
    Library::write_source_processing(
        &store,
        &captured.id,
        &sha256_hex(markdown.as_bytes()),
        Some("Updated transcript"),
        &DerivedBy {
            route: ModelRoute::Hosted,
            model: "fixture".into(),
        },
        &[],
    )
    .unwrap();
    assert!(
        office_core::domain::sharing::shareable(
            &store.read_text(Path::new(&captured.path)).unwrap()
        )
        .unwrap()
    );
    assert!(store.get(&captured.id).unwrap().unwrap().shareable);
    store.reindex().unwrap();
    assert!(store.get(&captured.id).unwrap().unwrap().shareable);
}

#[test]
fn sharing_reports_a_deleted_indexed_document_as_missing_and_a_missing_cite_as_unshared() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    let path = Path::new("sources/2026/10/src-deleted/source.md");
    store
        .write_document(
            path,
            "---\nid: src-deleted\ntitle: Deleted\nshareable: true\n---\nBody.\n",
        )
        .unwrap();
    let page_path = Path::new("knowledge/domain/dom-deleted.md");
    store
        .write_document(
            page_path,
            "---\nid: dom-deleted\ntitle: Deleted evidence\ncites: [src-deleted#L1]\n---\nBody.\n",
        )
        .unwrap();
    std::fs::remove_file(root.join(path)).unwrap();
    let head = store.head_commit().unwrap();
    assert_eq!(
        store
            .set_shareable("src-deleted", false)
            .unwrap_err()
            .code(),
        "document_not_found"
    );
    assert_eq!(
        store.set_shareable("dom-deleted", true).unwrap_err().code(),
        "share_cites_unshared"
    );
    assert_eq!(store.head_commit().unwrap(), head);
}
