//! Real LibraryStore capture: containment, dedupe and append commits.

use office_core::ports::{CaptureSourceRequest, ConversationEntry};

use crate::LibraryStore;

fn request() -> CaptureSourceRequest {
    CaptureSourceRequest {
        captured_at: "2026-10-27T10:00:00+00:00".to_string(),
        ..CaptureSourceRequest::default()
    }
}

fn setup() -> (tempfile::TempDir, LibraryStore, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let library = workspace.join("library");
    let state = dir.path().join("state");
    LibraryStore::initialize_at(&library, &state, None, &workspace).expect("init");
    let store = LibraryStore::open(&library, &state).expect("open");
    (dir, store, workspace)
}

#[test]
fn capture_upload_symlink_escape_is_refused_and_writes_nothing() {
    let (dir, store, workspace) = setup();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "secret-ish").expect("outside");
    let link = workspace.join("escape.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    let mut req = request();
    req.upload_path = Some(link.to_string_lossy().to_string());
    req.title = Some("Escape".to_string());
    let error = store.capture_source(&req).expect_err("escape refused");
    assert_eq!(error.code(), "upload_outside_allowed_roots");
    assert_eq!(store.document_count().expect("count"), 0);
}

#[test]
fn capture_conversation_append_commits_once_per_append_and_preserves_lines() {
    let (_dir, store, _workspace) = setup();
    let mut req = request();
    req.kind = Some("conversation".to_string());
    req.title = Some("Session".to_string());
    req.entries = vec![ConversationEntry {
        kind: "decision".to_string(),
        text: "First.".to_string(),
    }];
    let opened = store.capture_source(&req).expect("open conversation");
    let before = store.read_source(&opened.id).expect("read");
    let mut append = request();
    append.append_to = Some(opened.id.clone());
    append.entries = vec![ConversationEntry {
        kind: "fact".to_string(),
        text: "Jane runs procurement.".to_string(),
    }];
    store.capture_source(&append).expect("append");
    let after = store.read_source(&opened.id).expect("read after");
    assert!(
        after.body.starts_with(&before.body),
        "earlier lines byte-identical"
    );
    assert!(after.body.contains("fact: Jane runs procurement."));
    let history = store
        .history(std::path::Path::new(&after.path))
        .expect("history");
    assert_eq!(history.len(), 2, "one commit per append: {history:?}");
}

#[test]
fn capture_same_upload_twice_dedupes_in_the_real_store() {
    let (_dir, store, workspace) = setup();
    let upload = workspace.join("note.txt");
    std::fs::write(&upload, "same text").expect("upload");
    let mut req = request();
    req.upload_path = Some(upload.to_string_lossy().to_string());
    req.title = Some("Note".to_string());
    let first = store.capture_source(&req).expect("first");
    let second = store.capture_source(&req).expect("second");
    assert_eq!(first.id, second.id);
    assert!(second.deduplicated);
    assert_eq!(store.document_count().expect("count"), 1);
}
