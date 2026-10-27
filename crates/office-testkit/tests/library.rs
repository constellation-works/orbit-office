//! Fake Library operations through the same public port as real adapters.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::{
    domain::knowledge::{DocumentKind, SearchFilter},
    ports::{CaptureSourceRequest, ConversationEntry, Library},
};
use office_testkit::fakes::FakeLibrary;
use std::io::Write as _;
use std::path::Path;

#[test]
fn fake_implements_read_write_filtered_search_history_settings_and_readiness() {
    let fake = FakeLibrary::initialized();
    let port: &dyn Library = &fake;
    let path = Path::new("knowledge/domain/example.md");
    assert!(port.is_initialized().expect("ready"));
    let document = port
        .write_document(
            path,
            "---\nid: dom-example\ntitle: Example\ntags: [client]\n---\nsearchable body",
        )
        .expect("write");
    assert_eq!(port.read_document(path).expect("read"), document);
    assert_eq!(port.history(path).expect("history").len(), 1);
    let filter = SearchFilter {
        kind: Some(DocumentKind::Domain),
        tag: Some("client".into()),
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(port.search("searchable", &filter).expect("search").len(), 1);
    assert!(port.search("absent", &filter).expect("no match").is_empty());
    port.settings().expect("valid settings");
    assert!(
        !FakeLibrary::default()
            .is_initialized()
            .expect("uninitialized")
    );
    assert!(FakeLibrary::default().read_document(path).is_err());
}

#[test]
fn fake_capture_append_updates_search_and_read_document() {
    let fake = FakeLibrary::initialized();
    let port: &dyn Library = &fake;
    let mut create = CaptureSourceRequest {
        captured_at: "2026-10-27T10:00:00+00:00".to_string(),
        ..CaptureSourceRequest::default()
    };
    create.kind = Some("conversation".to_string());
    create.title = Some("Session log".to_string());
    create.tags = vec!["bootstrap:first".to_string()];
    create.entries = vec![ConversationEntry {
        kind: "fact".to_string(),
        text: "Jane runs procurement.".to_string(),
    }];
    let opened = port.capture_source(&create).expect("open conversation");

    let mut append = CaptureSourceRequest {
        captured_at: "2026-10-27T10:05:00+00:00".to_string(),
        ..CaptureSourceRequest::default()
    };
    append.append_to = Some(opened.id.clone());
    append.tags = vec!["bootstrap:second".to_string()];
    append.entries = vec![ConversationEntry {
        kind: "decision".to_string(),
        text: "Record appendeduniqueterm for later.".to_string(),
    }];
    let appended = port.capture_source(&append).expect("append entry");
    assert_eq!(appended.appended, 1);
    assert!(
        port.read_source(&opened.id)
            .expect("read source")
            .body
            .contains("appendeduniqueterm")
    );

    let hits = port
        .search("appendeduniqueterm", &SearchFilter::default())
        .expect("search");
    assert_eq!(hits.len(), 1, "search should find appended entry: {hits:?}");
    assert_eq!(hits[0].id, opened.id);
    assert_eq!(hits[0].path, opened.path);

    let document = port
        .read_document(Path::new(&opened.path))
        .expect("read document");
    assert_eq!(document.tags, ["bootstrap:first", "bootstrap:second"]);
    assert!(
        document.body.contains("appendeduniqueterm"),
        "read_document body should contain appended term: {}",
        document.body
    );
}

#[test]
fn fake_capture_upload_then_paste_dedupes_to_the_same_id() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let fixture_dir = dir.path().to_owned();
    let upload = dir.path().join("notes-crlf.txt");
    std::fs::File::create(&upload)
        .expect("create upload")
        .write_all(b"Hello world.  \r\nSecond line.\r\n")
        .expect("write upload");

    let fake = FakeLibrary::initialized();
    let port: &dyn Library = &fake;
    let uploaded = port
        .capture_source(&CaptureSourceRequest {
            upload_path: Some(upload.to_string_lossy().to_string()),
            title: Some("CRLF upload".to_string()),
            captured_at: "2026-10-27T10:00:00+00:00".to_string(),
            ..CaptureSourceRequest::default()
        })
        .expect("capture upload");
    assert!(!uploaded.deduplicated);

    let pasted = port
        .capture_source(&CaptureSourceRequest {
            text: Some("Hello world.\nSecond line.".to_string()),
            title: Some("Pasted copy".to_string()),
            captured_at: "2026-10-27T10:01:00+00:00".to_string(),
            ..CaptureSourceRequest::default()
        })
        .expect("capture paste");
    assert!(
        pasted.deduplicated,
        "same normalized text pasted after a CRLF upload should deduplicate: {pasted:?}"
    );
    assert_eq!(pasted.id, uploaded.id);
    drop(dir);
    assert!(
        !fixture_dir.exists(),
        "upload fixture directory should be removed after use"
    );
}

#[test]
fn fake_initialize_refuses_a_library_dir_outside_the_granted_root() {
    for outside in [
        "/etc/library/evil",
        "a/library/b",
        "/library",
        "library/../outside",
    ] {
        let fake = FakeLibrary::default();
        let port: &dyn Library = &fake;
        let err = port
            .initialize(Some(outside))
            .expect_err("a library_dir outside the granted root is refused");
        assert_eq!(err.code(), "library_dir_outside_workspace", "{outside}");
        assert!(
            !port.is_initialized().expect("uninitialized"),
            "a refused {outside} must not scaffold the Library"
        );
    }
}

#[test]
fn fake_initialize_accepts_the_library_and_workspace_relative_library_dirs() {
    for inside in ["library", "library/sub", "./library"] {
        let fake = FakeLibrary::default();
        let port: &dyn Library = &fake;
        let outcome = port
            .initialize(Some(inside))
            .expect("a workspace-relative library dir initializes");
        assert!(!outcome.already_initialized, "{inside}");
        assert!(port.is_initialized().expect("initialized"));
    }
}
