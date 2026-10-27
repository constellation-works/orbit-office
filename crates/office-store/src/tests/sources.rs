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
fn source_documents_enumerate_all_full_documents_without_the_search_limit() {
    use office_core::ports::Library;

    let (_dir, store, workspace) = setup();
    let root = workspace.join("library/sources/1970-02");
    let mut expected = Vec::new();
    for index in 0..105 {
        let name = format!("src-{index:03}");
        let directory = root.join(&name);
        std::fs::create_dir_all(&directory).expect("source directory");
        let markdown = format!(
            "---\nid: {name}\nkind: text\nparticipants: [Priya]\ncaptured_at: 1970-02-01\n---\n{}\nrhythm: weekly briefing",
            "context ".repeat(30)
        );
        std::fs::write(directory.join("source.md"), &markdown).expect("source");
        std::fs::write(directory.join("original.md"), "not a source").expect("original");
        std::fs::write(directory.join("v2.md"), "not the current source").expect("sidecar");
        expected.push(office_core::ports::LibraryText {
            path: format!("sources/1970-02/{name}/source.md"),
            markdown: Some(markdown),
        });
    }
    // No reindex is needed: enumeration reads the files, not search hits.
    let port: &dyn Library = &store;
    assert_eq!(
        port.source_documents().expect("all complete sources"),
        expected
    );
}

#[test]
fn source_documents_refuse_unreadable_markdown_instead_of_omitting_it() {
    let (_dir, store, workspace) = setup();
    let directory = workspace.join("library/sources/bad");
    std::fs::create_dir_all(&directory).expect("source directory");
    std::fs::write(directory.join("source.md"), [0xff, 0xfe]).expect("invalid UTF-8 source");
    let error = store
        .source_documents()
        .expect_err("refuse incomplete read");
    assert_eq!(error.code(), "io_error");
}

#[cfg(unix)]
#[test]
fn source_documents_do_not_follow_directory_file_or_ancestor_symlinks() {
    use std::os::unix::fs::symlink;

    let (dir, store, workspace) = setup();
    let sources = workspace.join("library/sources");
    let inside = sources.join("inside");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&inside).expect("inside");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(inside.join("source.md"), "inside").expect("source");
    std::fs::write(outside.join("source.md"), "outside").expect("outside source");
    symlink(&outside, sources.join("directory-link")).expect("directory symlink");
    let file_link = sources.join("file-link");
    std::fs::create_dir(&file_link).expect("file-link directory");
    symlink(outside.join("source.md"), file_link.join("source.md")).expect("file symlink");
    symlink(&sources, inside.join("cycle")).expect("ancestor cycle");
    let documents = store.source_documents().expect("scan terminates");
    assert_eq!(
        documents,
        [office_core::ports::LibraryText {
            path: "sources/inside/source.md".into(),
            markdown: Some("inside".into()),
        }]
    );
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

#[test]
fn capture_read_and_dedupe_preserve_title_with_quotes_and_backslashes() {
    let (_dir, store, workspace) = setup();
    let upload = workspace.join("note.txt");
    std::fs::write(&upload, "same text").expect("upload");
    let title = r#"Say "Hello" from C:\docs"#;
    let mut req = request();
    req.upload_path = Some(upload.to_string_lossy().to_string());
    req.title = Some(title.to_string());
    let first = store.capture_source(&req).expect("first");
    assert_eq!(first.title, title);
    let read = store.read_source(&first.id).expect("read");
    assert_eq!(read.title, title);
    let second = store.capture_source(&req).expect("second");
    assert!(second.deduplicated);
    assert_eq!(second.id, first.id);
    assert_eq!(second.title, title);
}

#[test]
fn capture_vtt_transcript_keeps_numeric_and_keyword_prefixed_cue_text() {
    let (_dir, store, workspace) = setup();
    let transcript = workspace.join("talk.vtt");
    std::fs::write(
        &transcript,
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n1234\n\n00:00:03.000 --> 00:00:04.000\nNOTE: the answer\n\n00:00:05.000 --> 00:00:06.000\nWEBVTT is the format\n",
    )
    .expect("transcript");
    let mut req = request();
    req.kind = Some("meeting".to_string());
    req.title = Some("Talk".to_string());
    req.transcript_path = Some(transcript.to_string_lossy().to_string());
    let outcome = store.capture_source(&req).expect("capture");
    let source = store.read_source(&outcome.id).expect("read");
    assert!(
        source.body.contains(
            "[00:00:01] Speaker: 1234\n\n[00:00:03] NOTE: the answer\n\n[00:00:05] Speaker: WEBVTT is the format"
        ),
        "{}",
        source.body
    );
}

#[test]
fn processing_persists_immutable_versions_sidecars_provenance_and_indexed_latest_body() {
    use office_core::domain::{endpoint::ModelRoute, source::sha256_hex};
    use office_core::ports::{DerivedBy, Library};
    let (_dir, store, workspace) = setup();
    let upload = workspace.join("meeting.wav");
    std::fs::write(&upload, b"recording").unwrap();
    let captured = store
        .capture_source(&CaptureSourceRequest {
            upload_path: Some(upload.to_string_lossy().into()),
            kind: Some("meeting".into()),
            title: Some("Meeting".into()),
            ..request()
        })
        .unwrap();
    let media = Library::read_media(&store, &captured.id).unwrap();
    assert_eq!(media.bytes, b"recording");
    let derived = DerivedBy {
        route: ModelRoute::Hosted,
        model: "fixture".into(),
    };
    let first = Library::write_source_processing(
        &store,
        &captured.id,
        &sha256_hex(media.markdown.as_bytes()),
        Some("[00:00:00] Ada: First.\n"),
        &derived,
        &[],
    )
    .unwrap();
    assert_eq!(first.version, 1);
    let directory = workspace
        .join("library")
        .join(std::path::Path::new(&first.path).parent().unwrap());
    let snapshot = std::fs::read(directory.join("v1.md")).unwrap();
    let markdown = store.read_text(std::path::Path::new(&first.path)).unwrap();
    let second=Library::write_source_processing(&store,&captured.id,&sha256_hex(markdown.as_bytes()),Some("[00:00:00] Ada: Corrected.\n"),&derived,&[("chapters.md".into(),b"## Chapter 1 [core]\n".to_vec()),("digest.json".into(),br#"{"summary":"Meeting","subjects":[],"claims":[],"procedures":[],"terms":[],"refs":[]}"#.to_vec())]).unwrap();
    assert_eq!(second.version, 2);
    assert_eq!(std::fs::read(directory.join("v1.md")).unwrap(), snapshot);
    assert!(
        std::fs::read_to_string(directory.join("v2.md"))
            .unwrap()
            .contains("supersedes: \"v1.md\"")
    );
    assert_eq!(
        Library::source_sidecar(&store, &captured.id, "chapters.md")
            .unwrap()
            .unwrap(),
        "## Chapter 1 [core]\n"
    );
    let record = store.get(&captured.id).unwrap().unwrap();
    assert!(record.body.contains("Corrected."));
    assert!(record.body.contains("Chapter 1"));
    assert!(
        Library::write_source_processing(&store, &captured.id, "stale", None, &derived, &[])
            .is_err()
    );
    assert!(Library::source_sidecar(&store, &captured.id, "../original.wav").is_err());
    assert!(
        Library::bootstrap_snapshot(&store).unwrap().sources[0]
            .digest
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn processing_refuses_symlinked_sidecars_and_versions_without_writing_outside() {
    use office_core::domain::{endpoint::ModelRoute, source::sha256_hex};
    use office_core::ports::{DerivedBy, Library};
    use std::os::unix::fs::symlink;
    let (dir, store, workspace) = setup();
    let source = store
        .capture_source(&CaptureSourceRequest {
            text: Some("Source".into()),
            ..request()
        })
        .unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"keep").unwrap();
    let parent = workspace
        .join("library")
        .join(std::path::Path::new(&source.path).parent().unwrap());
    symlink(&outside, parent.join("chapters.md")).unwrap();
    assert!(Library::source_sidecar(&store, &source.id, "chapters.md").is_err());
    let markdown = store.read_text(std::path::Path::new(&source.path)).unwrap();
    let derived = DerivedBy {
        route: ModelRoute::Hosted,
        model: "fixture".into(),
    };
    assert!(
        Library::write_source_processing(
            &store,
            &source.id,
            &sha256_hex(markdown.as_bytes()),
            None,
            &derived,
            &[("chapters.md".into(), b"replace".to_vec())]
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    assert_eq!(
        store.read_text(std::path::Path::new(&source.path)).unwrap(),
        markdown
    );
    symlink(&outside, parent.join("v1.md")).unwrap();
    assert!(
        Library::write_source_processing(
            &store,
            &source.id,
            &sha256_hex(markdown.as_bytes()),
            Some("Turn"),
            &derived,
            &[]
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
}
