//! Scribe SCR files in the Library store (specs/scribe.md): committed, not
//! indexed, refused on a stale read, and a journaled rename replays once.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_core::ports::{Library, ScribeWrite};
use office_store::LibraryStore;

const OLD: &str = "sources/conversation/2026/11/SCR-01-0300-first-words.md";
const NEW: &str = "sources/conversation/2026/11/SCR-01-0300-a-title.md";

fn write(path: &str, text: &str, previous: Option<(&str, &str)>) -> ScribeWrite {
    ScribeWrite {
        path: path.into(),
        text: text.into(),
        previous: previous.map(|(p, t)| (p.into(), t.into())),
    }
}

#[test]
fn scr_files_commit_without_indexing_and_refuse_stale_or_unsafe_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    let before = store.document_count().unwrap();

    Library::write_scribe_file(&store, &write(OLD, "---\nid: a\n---\n", None)).unwrap();
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(store.reindex().unwrap(), before);
    let files = Library::scribe_files(&store).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, OLD);

    // Creating over an existing file, or updating from a stale read, is refused.
    let error = Library::write_scribe_file(&store, &write(OLD, "x", None)).unwrap_err();
    assert_eq!(error.code(), "scribe_changed");
    let error =
        Library::write_scribe_file(&store, &write(OLD, "x", Some((OLD, "old text")))).unwrap_err();
    assert_eq!(error.code(), "scribe_changed");
    // Only SCR-shaped paths under sources/conversation/<yyyy>/<mm>/.
    for path in [
        "sources/conversation/2026/11/notes.md",
        "sources/conversation/2026/SCR-01-0300-x.md",
        "knowledge/domain/SCR-01-0300-x.md",
    ] {
        let error = Library::write_scribe_file(&store, &write(path, "x", None)).unwrap_err();
        assert_eq!(error.code(), "scribe_path_invalid", "{path}");
    }
    assert_eq!(
        std::fs::read_to_string(root.join(OLD)).unwrap(),
        "---\nid: a\n---\n"
    );
}

#[test]
fn an_interrupted_scr_rename_replays_once_on_open() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    Library::write_scribe_file(&store, &write(OLD, "old", None)).unwrap();
    drop(store);
    let pending = serde_json::json!({"version":10,"kind":"scribe","root":root.canonicalize().unwrap(),"relative":NEW,"text":"renamed","token":"123-789","removed":[OLD]});
    std::fs::write(
        state.join("library.pending-write.json"),
        pending.to_string(),
    )
    .unwrap();
    let store = LibraryStore::open(&root, &state).unwrap();
    assert_eq!(std::fs::read_to_string(root.join(NEW)).unwrap(), "renamed");
    assert!(!root.join(OLD).exists());
    assert!(!state.join("library.pending-write.json").exists());
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&root, &["ls-files", "sources/conversation"]).trim(),
        NEW
    );
    drop(store);
    // A scribe journal without its removals, or naming a non-SCR path, is refused.
    for pending in [
        serde_json::json!({"version":10,"kind":"scribe","root":root.canonicalize().unwrap(),"relative":NEW,"text":"x","token":"1-2"}),
        serde_json::json!({"version":10,"kind":"scribe","root":root.canonicalize().unwrap(),"relative":"knowledge/domain/x.md","text":"x","token":"1-2","removed":[]}),
    ] {
        std::fs::write(
            state.join("library.pending-write.json"),
            pending.to_string(),
        )
        .unwrap();
        assert!(
            LibraryStore::open(&root, &state)
                .and_then(|store| store.scribe_files())
                .is_err()
        );
    }
}
