//! Symlink and grant refusal scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::{LibraryStore, SearchFilter};
use std::path::Path;

#[test]
fn original_markdown_and_symlink_paths_are_refused_before_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    let store = LibraryStore::open(&library, &dir.path().join("state")).expect("store");
    assert_eq!(
        store
            .write_document(Path::new("sources/original.md"), "raw")
            .unwrap_err()
            .code(),
        "unsafe_path"
    );
    assert_eq!(
        store
            .write_document(Path::new("../escape.md"), "escape")
            .unwrap_err()
            .code(),
        "unsafe_path"
    );
    #[cfg(unix)]
    {
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, library.join("knowledge/link")).expect("symlink");
        assert_eq!(
            store
                .write_document(Path::new("knowledge/link/escape.md"), "escape")
                .unwrap_err()
                .code(),
            "unsafe_path"
        );
        assert!(!outside.join("escape.md").exists());
    }
}

#[cfg(unix)]
#[test]
fn init_rejects_symlinked_destinations_outside_the_grant_before_writing() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let library = workspace.join("library");
    let outside = dir.path().join("outside");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&library).expect("library grant");
    std::fs::create_dir_all(&outside).expect("outside");
    symlink(&outside, library.join("escape")).expect("symlink");

    for requested in ["library/escape/new", "library/escape"] {
        let result = LibraryStore::initialize_at(&library, &state, Some(requested), &workspace);
        assert!(result.is_err(), "accepted {requested}");
        assert_eq!(std::fs::read_dir(&outside).expect("outside").count(), 0);
        assert!(!state.join("index.sqlite").exists());
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_grant_root_is_refused_wherever_it_points() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let library = workspace.join("library");
    let state = dir.path().join("state");
    // Outside the workspace, inside it, and dangling.
    let targets = [
        dir.path().join("outside"),
        workspace.join("library-real"),
        dir.path().join("missing"),
    ];
    for target in &targets {
        std::fs::create_dir_all(&workspace).expect("workspace");
        if target.file_name() != Some(std::ffi::OsStr::new("missing")) {
            std::fs::create_dir_all(target).expect("target");
        }
        symlink(target, &library).expect("symlink grant root");

        let nested = library.join("subdir");
        let err = LibraryStore::confine_to_grant(&nested, &workspace).expect_err("nested");
        assert_eq!(
            err.code(),
            "library_dir_outside_workspace",
            "{}",
            target.display()
        );
        let err = LibraryStore::initialize_at(&library, &state, Some("library/subdir"), &workspace)
            .expect_err("init nested");
        assert_eq!(
            err.code(),
            "library_dir_outside_workspace",
            "{}",
            target.display()
        );
        assert!(LibraryStore::confine_to_grant(&library, &workspace).is_err());
        assert!(!target.join("subdir").exists(), "{}", target.display());
        assert!(!state.exists(), "{}", target.display());
        std::fs::remove_file(&library).expect("unlink grant root");
    }

    // A real library directory under a linked workspace is still granted.
    let real = dir.path().join("real-workspace");
    std::fs::create_dir_all(real.join("library")).expect("real library");
    let linked = dir.path().join("linked-workspace");
    symlink(&real, &linked).expect("symlink workspace");
    LibraryStore::confine_to_grant(&linked.join("library/subdir"), &linked)
        .expect("physical library under a linked workspace");
}

#[cfg(unix)]
#[test]
fn reindex_does_not_index_symlinks_outside_the_library() {
    let (_dir, store) = library_with_links_outside(true);
    let indexed = store.reindex().expect("reindex");
    assert_eq!(indexed, 2, "symlinked files and directories were indexed");
    assert_eq!(store.document_count().expect("count"), 2);
    for id in [
        "dom-outside-file",
        "dom-outside-dir",
        "src-outside-file",
        "src-outside-dir",
        "art-outside",
    ] {
        assert!(store.get(id).expect("get").is_none(), "{id} was indexed");
    }
    assert!(store.get("dom-inside").expect("inside page").is_some());
    assert!(store.get("src-inside").expect("inside source").is_some());
    assert!(
        store
            .search("OUTSIDE", &SearchFilter::default())
            .expect("search")
            .is_empty(),
        "search exposed text from outside the Library"
    );
}

#[cfg(unix)]
#[test]
fn knowledge_pages_do_not_read_symlinks_outside_the_library() {
    let (_dir, store) = library_with_links_outside(false);
    let pages = store
        .knowledge_pages()
        .expect("directory symlink must not abort the scan");
    assert!(
        pages.iter().all(|page| {
            page.markdown
                .as_deref()
                .is_none_or(|text| !text.contains("OUTSIDE"))
        }),
        "knowledge pages read through a directory symlink: {pages:?}"
    );
    assert!(
        pages
            .iter()
            .any(|page| page.path == "knowledge/domain/inside.md"),
        "in-library page missing: {pages:?}"
    );

    let (_dir, store) = library_with_links_outside(true);
    let pages = store
        .knowledge_pages()
        .expect("file symlink must not abort the scan");
    assert_eq!(
        pages.len(),
        1,
        "symlinked knowledge file was returned: {pages:?}"
    );
    assert_eq!(pages[0].path, "knowledge/domain/inside.md");
    assert!(
        pages[0]
            .markdown
            .as_deref()
            .unwrap()
            .contains("inside library page")
    );
}

#[cfg(unix)]
#[test]
fn reindex_and_knowledge_pages_terminate_on_an_ancestor_symlink_cycle() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "knowledge/domain/inside.md",
        "---\nid: dom-inside\ntype: domain\ntitle: Inside page\n---\n\ninside library page\n",
    );
    let store = LibraryStore::open(&library, &state).expect("open");
    symlink(library.join("knowledge"), library.join("knowledge/loop")).expect("ancestor cycle");
    let indexed = store.reindex().expect("reindex terminates");
    assert_eq!(indexed, 1);
    let pages = store.knowledge_pages().expect("knowledge pages terminate");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].path, "knowledge/domain/inside.md");
    assert!(
        pages[0]
            .markdown
            .as_deref()
            .unwrap()
            .contains("inside library page")
    );
}
