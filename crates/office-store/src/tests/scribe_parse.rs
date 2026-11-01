//! Parsed scribe files against a temporary workspace: they land under the
//! Library's `.office/state/scribe/`, their path is workspace-relative for
//! any Library directory, and a Library outside the workspace or a linked
//! state directory is refused.

use crate::Error;
use crate::scribe_parse::ScribeParseStore;

#[test]
fn files_land_under_the_library_state_and_read_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let library = workspace.join("library/./office");
    let store = ScribeParseStore::new(&workspace, &library).expect("store");
    assert_eq!(store.read("s1").expect("read"), None);
    let parsed = store.write("s1", "{}\n", "{\"a\":1}").expect("write");
    assert_eq!(parsed, "library/office/.office/state/scribe/s1.jsonl");
    assert_eq!(
        std::fs::read_to_string(workspace.join(&parsed)).expect("jsonl"),
        "{}\n"
    );
    let read = store.read("s1").expect("read").expect("present");
    assert_eq!(
        (read.messages.as_str(), read.meta.as_str()),
        ("{}\n", "{\"a\":1}")
    );
    assert_eq!(
        store.relative_path("s1").expect("path"),
        "library/office/.office/state/scribe/s1.jsonl"
    );
}

#[test]
fn a_library_outside_the_workspace_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let refused = ScribeParseStore::new(&workspace, &workspace.join("library/../../outside"));
    assert!(matches!(refused, Err(Error::LibraryDirOutside { .. })));
}

#[cfg(unix)]
#[test]
fn a_linked_state_directory_is_refused_and_nothing_is_written_through_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let library = workspace.join("library");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(library.join(".office")).expect("library");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    std::os::unix::fs::symlink(&elsewhere, library.join(".office/state")).expect("link");
    let store = ScribeParseStore::new(&workspace, &library).expect("store");
    let refused = store.write("s1", "{}\n", "{}");
    assert!(
        matches!(&refused, Err(office_core::Error::Library { code, .. }) if *code == "unsafe_path"),
        "{refused:?}"
    );
    assert!(store.read("s1").is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).expect("list").count(), 0);
}
