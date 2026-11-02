//! Decision ledger batches in the Library store (decision-precedents.md,
//! Ingesting Orbit decisions): every planned ledger written and committed
//! once, compare-and-set against the text read, and a journaled batch
//! replays once on open.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_core::domain::source::conversation::scribe::v2::PlannedFile;
use office_core::domain::source::decisions::{Ledger, Scope};
use office_core::ports::Library;
use office_store::LibraryStore;

fn empty(scope: Scope) -> (String, String) {
    let ledger = Ledger::new(scope, "2026-11-01T00:00:00Z");
    (ledger.path.clone(), ledger.render().unwrap())
}

#[test]
fn a_batch_writes_every_ledger_in_one_commit_and_refuses_a_stale_or_foreign_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    let commits = |root: &std::path::Path| git(root, &["rev-list", "--count", "HEAD"]);
    let before = commits(&root);

    let (global, global_text) = empty(Scope::Global);
    let (acme, acme_text) = empty(Scope::Project("acme".into()));
    let files = vec![
        PlannedFile {
            path: global.clone(),
            text: global_text.clone(),
            expected: None,
        },
        PlannedFile {
            path: acme.clone(),
            text: acme_text.clone(),
            expected: None,
        },
    ];
    Library::write_decision_ledgers(&store, &files).unwrap();
    assert_eq!(
        commits(&root).trim().parse::<u32>().unwrap(),
        before.trim().parse::<u32>().unwrap() + 1
    );
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(Library::decision_ledgers(&store).unwrap().len(), 2);

    // Planned against an absent file that now exists: refused, unchanged.
    let after = commits(&root);
    let error = Library::write_decision_ledgers(&store, &files).unwrap_err();
    assert_eq!(error.code(), "decision_ledger_changed");
    let foreign = PlannedFile {
        path: "knowledge/domain/dom-x.md".into(),
        text: "---\nid: dom-x\n---\n".into(),
        expected: None,
    };
    let error = Library::write_decision_ledgers(&store, &[foreign]).unwrap_err();
    assert_eq!(error.code(), "invalid_argument");
    Library::write_decision_ledgers(&store, &[]).unwrap();
    assert_eq!(
        commits(&root),
        after,
        "refusals and an empty batch commit nothing"
    );
}

#[test]
fn an_interrupted_batch_replays_once_on_open_and_a_foreign_path_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    drop(store);
    let (global, global_text) = empty(Scope::Global);
    let (acme, acme_text) = empty(Scope::Project("acme".into()));
    let bundle = serde_json::json!([{"path": acme, "bytes": acme_text.as_bytes()}]);
    let pending = serde_json::json!({"version":12,"kind":"decision_ledgers","root":root.canonicalize().unwrap(),"relative":global,"text":global_text,"token":"123-789","bundle":bundle});
    std::fs::write(
        state.join("library.pending-write.json"),
        pending.to_string(),
    )
    .unwrap();
    let store = LibraryStore::open(&root, &state).unwrap();
    assert_eq!(Library::decision_ledgers(&store).unwrap().len(), 2);
    assert!(!state.join("library.pending-write.json").exists());
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    drop(store);
    let foreign = serde_json::json!([{"path": "knowledge/domain/dom-x.md", "bytes": b"---\nid: dom-x\n---\n"}]);
    let pending = serde_json::json!({"version":12,"kind":"decision_ledgers","root":root.canonicalize().unwrap(),"relative":global,"text":global_text,"token":"1-2","bundle":foreign});
    std::fs::write(
        state.join("library.pending-write.json"),
        pending.to_string(),
    )
    .unwrap();
    assert!(
        LibraryStore::open(&root, &state)
            .and_then(|store| store.decision_ledger_files())
            .is_err()
    );
}
