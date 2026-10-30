//! Artifact, video and digest scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::{LibraryStore, SearchFilter};
use std::path::Path;

#[test]
fn artifact_promotion_commits_the_complete_bundle_and_keeps_every_revision() {
    use office_core::domain::artifact::Artifact;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let state = dir.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    // Unrelated staged content must stay out of the artifact commit.
    std::fs::write(root.join("unrelated.txt"), "preserve").unwrap();
    git(&root, &["add", "unrelated.txt"]);
    let first = store
        .promote_artifact(&artifact_candidate(), &artifact_files())
        .unwrap();
    let path = first.path().unwrap();
    let parent = Path::new(&path).parent().unwrap();
    assert_eq!(
        std::fs::read(root.join(parent).join("data/results.bin")).unwrap(),
        [0, 255, 128, 3]
    );
    assert_eq!(
        Artifact::from_markdown(&store.read_text(Path::new(&path)).unwrap()).unwrap(),
        first
    );
    assert_eq!(first.produced_by.crew.as_deref(), Some("fixture-crew"));
    assert!(first.approved.edited);
    assert_eq!(first.version, 1);
    let committed = git(&root, &["show", "--format=", "--name-only", "HEAD"]);
    let mut names: Vec<_> = committed.lines().filter(|s| !s.is_empty()).collect();
    names.sort();
    let mut expected = vec![
        path.clone(),
        format!("{}/report.md", parent.display()),
        format!("{}/data/results.bin", parent.display()),
    ];
    expected.sort();
    assert_eq!(names, expected);
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        git(&root, &["diff", "--cached", "--name-only"]).trim(),
        "unrelated.txt"
    );
    let original = std::fs::read(root.join(&path)).unwrap();
    let next = store
        .promote_artifact(&artifact_candidate(), &artifact_files())
        .unwrap();
    assert_ne!(next.id, first.id);
    assert_eq!(next.version, 2);
    assert_eq!(next.supersedes, Some(first.id.clone()));
    assert_eq!(std::fs::read(root.join(&path)).unwrap(), original);
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]).trim(), "2");
    store.reindex().unwrap();
    assert_eq!(store.document_count().unwrap(), 2);
    assert_eq!(
        store.get(&next.id).unwrap().unwrap().series.as_deref(),
        Some("fixture-series")
    );
    assert_eq!(store.artifact_documents().unwrap().len(), 2);
}

#[test]
fn artifact_journal_recovers_partial_publication_and_never_duplicates_the_commit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let state = dir.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    let a = artifact_candidate();
    let path = a.path().unwrap();
    let text = a.to_markdown().unwrap();
    let parent = Path::new(&path).parent().unwrap();
    let staging = root
        .join(parent.parent().unwrap())
        .join(".office-pending-artifact-777-111");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("report.md"), b"Approved report\n").unwrap();
    assert!(!root.join(&path).exists());
    assert_eq!(store.reindex().unwrap(), 0);
    let journal = state.join("library.pending-write.json");
    let decision = serde_json::json!({"version":4,"kind":"artifact","root":root.canonicalize().unwrap(),"relative":path,"text":text,"token":"777-111","bundle":artifact_files()});
    std::fs::write(&journal, serde_json::to_vec(&decision).unwrap()).unwrap();
    drop(store);
    let recovered = LibraryStore::open(&root, &state).unwrap();
    assert_eq!(
        recovered.get(&a.id).unwrap().unwrap().series.as_deref(),
        Some("fixture-series")
    );
    assert_eq!(
        std::fs::read(root.join(parent).join("data/results.bin")).unwrap(),
        [0, 255, 128, 3]
    );
    assert!(!journal.exists());
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    // A crash after commit, before removing the journal, is also idempotent.
    std::fs::write(&journal, serde_json::to_vec(&decision).unwrap()).unwrap();
    drop(recovered);
    LibraryStore::open(&root, &state).unwrap();
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert!(!journal.exists());
}

#[test]
fn artifact_bundle_refusals_leave_files_index_and_commits_unchanged() {
    use office_core::domain::artifact::Approver;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let state = dir.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    let mut a = artifact_candidate();
    a.approved.by = Approver::Routine;
    assert_eq!(
        store
            .promote_artifact(&a, &artifact_files())
            .unwrap_err()
            .code(),
        "artifact_routine_approval_refused"
    );
    a.approved.by = Approver::User;
    let mut files = artifact_files();
    files[1].path = "../../outside".into();
    assert!(store.promote_artifact(&a, &files).is_err());
    assert_eq!(store.document_count().unwrap(), 0);
    assert_eq!(store.head_commit().unwrap(), None);
    assert!(!state.join("library.pending-write.json").exists());
    assert!(!root.join(a.path().unwrap()).exists());
}

#[test]
fn derived_video_text_is_indexed_under_the_source_id() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    initialize(&library);
    let parent = "sources/2026/10/src-20261028-acme-qbr";
    write(&library, &format!("{parent}/source.md"), MEETING);
    write(
        &library,
        &format!("{parent}/frames.md"),
        "[00:00:10] Quasar rotation panel #frame=00:00:10",
    );
    write(
        &library,
        &format!("{parent}/chapters.md"),
        "Neutrino maintenance chapter",
    );
    let store = LibraryStore::open(&library, &dir.path().join("state")).unwrap();
    assert_eq!(store.reindex().unwrap(), 1);
    for phrase in ["Quasar", "Neutrino"] {
        let hits = store.search(phrase, &SearchFilter::default()).unwrap();
        assert_eq!(hits.len(), 1, "{phrase}");
        assert_eq!(hits[0].id, "src-20261028-acme-qbr");
    }
}

#[test]
fn video_result_journal_recovers_markdown_image_budget_and_source_index_together() {
    use office_core::domain::endpoint::ModelRoute;
    use office_core::domain::source::{sha256_hex, video};
    use office_core::ports::{CaptureSourceRequest, DerivedBy, Library};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let state_dir = dir.path().join("state");
    let upload = dir.path().join("video.mp4");
    std::fs::write(&upload, b"video").unwrap();
    let store = LibraryStore::open(&root, &state_dir).unwrap();
    let captured = store
        .capture_source(&CaptureSourceRequest {
            upload_path: Some(upload.to_string_lossy().into()),
            captured_at: "1970-01-01T00:00:00Z".into(),
            ..Default::default()
        })
        .unwrap();
    let source = store.read_video(&captured.id).unwrap();
    let plan = video::plan(
        &source,
        &video::VideoAnalysis {
            duration_ms: 60000,
            has_audio: false,
            candidates: vec![video::VideoFrame {
                time_ms: 10000,
                score: 800000,
                dhash: 0,
            }],
        },
        &store.settings().unwrap(),
        "normal",
    )
    .unwrap();
    store
        .mutate_video(
            &captured.id,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::Plan {
                plan: Box::new(plan.clone()),
                confirm_digest: None,
                at: "1970-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
    let source = store.read_video(&captured.id).unwrap();
    store
        .mutate_video(
            &captured.id,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::Plan {
                plan: Box::new(plan.clone()),
                confirm_digest: Some(plan.digest.clone()),
                at: "1970-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
    let source = store.read_video(&captured.id).unwrap();
    store
        .mutate_video(
            &captured.id,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::Reserve {
                digest: plan.digest.clone(),
                index: 0,
            },
        )
        .unwrap();
    let source = store.read_video(&captured.id).unwrap();
    let write = video::apply(
        &source,
        &store.settings().unwrap(),
        video::VideoMutation::Frame {
            digest: plan.digest,
            index: 0,
            image: b"PNG fixture".to_vec(),
            text: "Quasar recovered frame".into(),
            derived_by: DerivedBy {
                route: ModelRoute::Hosted,
                model: "fixture".into(),
            },
        },
    )
    .unwrap();
    let files = write
        .files
        .into_iter()
        .map(|(path, bytes)| office_core::domain::artifact::ArtifactFile { path, bytes })
        .collect::<Vec<_>>();
    let decision = serde_json::json!({"version":5,"kind":"video","root":root.canonicalize().unwrap(),"relative":source.path,"text":write.markdown,"token":"777-222","bundle":files});
    std::fs::write(
        state_dir.join("library.pending-write.json"),
        serde_json::to_vec(&decision).unwrap(),
    )
    .unwrap();
    drop(store);
    let recovered = LibraryStore::open(&root, &state_dir).unwrap();
    let source = recovered.read_video(&captured.id).unwrap();
    let state = video::state(&source.markdown).unwrap().unwrap();
    assert_eq!(state.frames_read, 1);
    assert_eq!(state.calls.frames, 1);
    let parent = Path::new(&source.path).parent().unwrap();
    assert_eq!(
        std::fs::read(root.join(parent).join("frames/0000.png")).unwrap(),
        b"PNG fixture"
    );
    assert_eq!(
        recovered
            .search("Quasar", &SearchFilter::default())
            .unwrap()[0]
            .id,
        captured.id
    );
    assert!(!state_dir.join("library.pending-write.json").exists());
    let mut unsafe_decision = decision;
    unsafe_decision["bundle"][0]["path"] = "../escape.png".into();
    std::fs::write(
        state_dir.join("library.pending-write.json"),
        serde_json::to_vec(&unsafe_decision).unwrap(),
    )
    .unwrap();
    drop(recovered);
    assert!(LibraryStore::open(&root, &state_dir).is_err());
    assert!(state_dir.join("library.pending-write.json").exists());
    assert!(!root.join(parent).join("../escape.png").exists());
}

#[test]
fn daily_digest_duplicate_candidates_cannot_publish_twice() {
    use office_core::domain::artifact::Approver;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let state = dir.path().join("state");
    let store = LibraryStore::open(&root, &state).unwrap();
    let mut candidate = artifact_candidate();
    candidate.series = "digest-daily".into();
    candidate.task = "routine:office-daily-digest/2026-10-26".into();
    candidate.approved.by = Approver::Routine;
    candidate.approved.edited = false;
    // Both callers can hold this same candidate before either publishes it.
    store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap();
    let head = store.head_commit().unwrap();
    let error = store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap_err();
    assert_eq!(error.code(), "digest_already_exists");
    assert_eq!(store.head_commit().unwrap(), head);
    assert_eq!(store.artifact_documents().unwrap().len(), 1);
    candidate.task = "routine:office-daily-digest/2026-10-27".into();
    let next = store
        .promote_artifact(&candidate, &artifact_files())
        .unwrap();
    assert_eq!(next.version, 2);
}

#[test]
fn artifact_publication_checks_sharing_before_any_bundle_write() {
    use office_core::domain::artifact::{
        Approval, Approver, Artifact, ArtifactFile, ArtifactManifest, Producer,
    };
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    initialize(&root);
    let store = LibraryStore::open(&root, &dir.path().join("state")).unwrap();
    store
        .write_document(
            Path::new("sources/2026/10/src-cited/source.md"),
            "---\nid: src-cited\ntitle: Cited evidence\n---\nLine.\n",
        )
        .unwrap();
    let manifest: ArtifactManifest = serde_json::from_value(json!({"kind":"report","title":"Shared artifact","series":"shared-artifact","files":["report.md"],"cites":["src-cited#L1"],"shareable":true})).unwrap();
    let artifact = Artifact::approved(
        manifest,
        "fixture-task".into(),
        Producer {
            crew: None,
            runbook: None,
            at: None,
        },
        Approval {
            by: Approver::User,
            at: "2026-10-28T00:00:00Z".into(),
            edited: false,
        },
    )
    .unwrap();
    let files = [ArtifactFile {
        path: "report.md".into(),
        bytes: b"Approved report".to_vec(),
    }];
    let head = store.head_commit().unwrap();
    assert_eq!(
        store
            .promote_artifact(&artifact, &files)
            .unwrap_err()
            .code(),
        "share_cites_unshared"
    );
    assert_eq!(store.head_commit().unwrap(), head);
    assert!(!root.join(artifact.path().unwrap()).exists());
    store.set_shareable("src-cited", true).unwrap();
    let published = store.promote_artifact(&artifact, &files).unwrap();
    let path = published.path().unwrap();
    assert!(
        Artifact::from_markdown(&store.read_text(Path::new(&path)).unwrap())
            .unwrap()
            .shareable
    );
    assert!(store.get(&published.id).unwrap().unwrap().shareable);
    assert_eq!(store.history(Path::new(&path)).unwrap().len(), 1);
    let mut private = artifact;
    private.sensitivity = office_core::ports::Sensitivity::Private;
    let head = store.head_commit().unwrap();
    assert_eq!(
        store.promote_artifact(&private, &files).unwrap_err().code(),
        "share_private_refused"
    );
    assert_eq!(store.head_commit().unwrap(), head);
}
