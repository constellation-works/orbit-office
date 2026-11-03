//! Legacy SCR files in the Library store. This build writes no SCR (the
//! scribe moved to the standalone scribe plugin), but SCR files an earlier
//! build wrote stay on disk: they are read in path order, never indexed and
//! never removed, and an interrupted SCR journal from an earlier build still
//! replays once on open, so no Library data is lost.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_core::ports::Library;
use office_store::LibraryStore;

const OLD: &str = "sources/conversation/2026/11/SCR-01-0300-first-words.md";
const NEW: &str = "sources/conversation/2026/11/SCR-01-0300-a-title.md";

/// Commit one SCR file the way an earlier build's scribe left it.
fn seed_scr(root: &std::path::Path, relative: &str, text: &str) {
    write(root, relative, text);
    git(root, &["add", relative]);
    git(root, &["commit", "-q", "-m", "seed SCR"]);
}

#[test]
fn existing_scr_files_are_read_untouched_and_not_indexed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    let before = store.document_count().unwrap();
    seed_scr(&root, OLD, "---\nid: a\n---\n");

    assert_eq!(store.reindex().unwrap(), before);
    let files = Library::scribe_files(&store).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, OLD);
    assert_eq!(files[0].markdown.as_deref(), Some("---\nid: a\n---\n"));
    drop(store);
    // Reopening and reindexing leave the file and the history alone.
    let store = LibraryStore::open(&root, &state).unwrap();
    store.reindex().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(OLD)).unwrap(),
        "---\nid: a\n---\n"
    );
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
}

#[test]
fn an_interrupted_scr_rename_from_an_earlier_build_replays_once_on_open() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let state = temp.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    Library::initialize(&store, None).unwrap();
    seed_scr(&root, OLD, "old");
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
