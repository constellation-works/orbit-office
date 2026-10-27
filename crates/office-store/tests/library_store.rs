//! End-to-end: a Library on disk, opened through the facade, reindexed
//! and searched — the flow `office.reindex` and `office.search` will use.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_store::{DocumentKind, Error, LibraryStore, SearchFilter, SearchHit};
use std::path::Path;

const MEETING: &str = "\
---
id: src-20261028-acme-qbr
kind: meeting
title: Acme QBR
summary: Renewal pricing discussion.
tags: [client:acme]
sensitivity: normal
---

[00:12:31] Jane Park: We cannot go above three percent this year.
";

const DOMAIN: &str = "\
---
id: dom-acme-renewal
type: domain
title: Acme renewal
summary: Acme's contract renews in March; Jane Park decides.
tags: [client:acme]
cites:
  - src-20261028-acme-qbr#t=00:12:31
related: [rb-client-qbr-prep]
---

Acme's contract renews in March [src-20261028-acme-qbr#t=00:12:31].
";

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

#[test]
fn reindex_then_search_and_get() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "sources/2026/10/src-20261028-acme-qbr/source.md",
        MEETING,
    );
    write(&library, "knowledge/domain/dom-acme-renewal.md", DOMAIN);
    // A non-document file in a source directory is not indexed.
    write(
        &library,
        "sources/2026/10/src-20261028-acme-qbr/v2.md",
        "correction draft",
    );

    let store = LibraryStore::open(&library, &state).expect("open store");
    assert!(store.index_path().ends_with("index.sqlite"));

    let indexed = store.reindex().expect("reindex");
    assert_eq!(indexed, 2);
    assert_eq!(store.document_count().expect("count"), 2);

    let hits = store
        .search("renewal", &SearchFilter::default())
        .expect("search");
    assert_eq!(hits.len(), 2);

    let sources = store
        .search(
            "pricing",
            &SearchFilter {
                kind: Some(DocumentKind::Source),
                ..SearchFilter::default()
            },
        )
        .expect("filtered search");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, "src-20261028-acme-qbr");

    let page = store
        .get("dom-acme-renewal")
        .expect("get")
        .expect("present");
    assert_eq!(page.kind, DocumentKind::Domain);
    assert_eq!(page.cites, vec!["src-20261028-acme-qbr#t=00:12:31"]);
}

#[test]
fn write_document_indexes_immediately_and_dedupes_by_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    let store = LibraryStore::open_in_memory(&library).expect("open in-memory store");

    let record = store
        .write_document(
            Path::new("knowledge/runbooks/rb-follow-up.md"),
            "---\nid: rb-follow-up\ntype: runbook\ntitle: Client follow-up\n---\n\nSteps.\n",
        )
        .expect("write document");
    assert_eq!(record.kind, DocumentKind::Runbook);
    assert_eq!(store.document_count().expect("count"), 1);

    let hash = store
        .file_sha256(Path::new("knowledge/runbooks/rb-follow-up.md"))
        .expect("hash written file");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        store
            .file_sha256(Path::new("knowledge/runbooks/rb-follow-up.md"))
            .expect("hash is stable")
    );
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.test");
    let (status, output) = office_common::process::run(&mut command, root).expect("git fixture");
    assert!(status.success(), "git fixture {args:?} exited {status}");
    output
}

fn initialize(root: &Path) {
    std::fs::create_dir_all(root.join(".office")).expect("config dir");
    std::fs::write(root.join(".office/config.yaml"), "").expect("config");
    git(root, &["init", "--quiet"]);
}

fn upload_request(root: &Path) -> office_core::ports::CaptureSourceRequest {
    let upload = root.join("upload.bin");
    std::fs::write(&upload, [0, 255, 1, 128, 2]).expect("binary upload");
    office_core::ports::CaptureSourceRequest {
        captured_at: "2026-10-27T10:00:00+00:00".into(),
        title: Some("Upload".into()),
        upload_path: Some(upload.to_string_lossy().into()),
        ..Default::default()
    }
}

const UPLOAD_DIR: &str = "sources/2026/10/src-20261027-upload";

#[cfg(unix)]
#[test]
fn upload_failed_decision_leaves_no_original_and_retry_succeeds() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    // Isolate PATH in a child. The git shim inserts a directory at the
    // journal path after recovery, during the initialized-repository check.
    // Atomic journal publication then fails deterministically, even as root.
    let Some(root) = std::env::var_os("OFFICE_TEST_UPLOAD_DECISION_ROOT") else {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let real_git = std::env::split_paths(&inherited_path)
            .map(|path| path.join("git"))
            .find(|path| path.is_file())
            .expect("git on PATH")
            .canonicalize()
            .unwrap();
        symlink(real_git, bin.join("git.real")).unwrap();
        let shim = bin.join("git");
        std::fs::write(
            &shim,
            "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = --show-toplevel ] && [ -f \"$HOME/fail-decision\" ]; then\n\
                 mkdir -p \"$HOME/library.pending-write.json\" || exit $?\n\
               fi\n\
             done\n\
             exec \"${0%/*}/git.real\" \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&inherited_path)),
        )
        .unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "upload_failed_decision_leaves_no_original_and_retry_succeeds",
                "--nocapture",
            ])
            .current_dir(dir.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", dir.path())
            .env("OFFICE_TEST_UPLOAD_DECISION_ROOT", dir.path());
        let (status, output) = office_common::process::run(&mut command, dir.path()).unwrap();
        assert!(
            status.success(),
            "decision fixture exited {status}: {output}"
        );
        return;
    };
    let root = Path::new(&root);
    let library = root.join("library");
    let state = root.join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let request = upload_request(root);
    std::fs::write(state.join("fail-decision"), "").unwrap();
    assert_eq!(
        store.capture_source(&request).unwrap_err().code(),
        "io_error"
    );
    assert!(state.join("library.pending-write.json").is_dir());
    assert!(
        std::fs::read_dir(library.join("sources"))
            .unwrap()
            .next()
            .is_none(),
        "a failed decision must not publish an original or source directory"
    );
    std::fs::remove_file(state.join("fail-decision")).unwrap();
    std::fs::remove_dir(state.join("library.pending-write.json")).unwrap();
    let captured = store.capture_source(&request).expect("retry capture");
    assert_eq!(captured.id, "src-20261027-upload");
    assert_eq!(store.document_count().unwrap(), 1);
}

#[test]
fn upload_interrupted_decision_recovers_original_and_replays_once() {
    for phase in ["decision", "original", "document"] {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let state = dir.path().join("state");
        initialize(&library);
        let store = LibraryStore::open(&library, &state).unwrap();
        let request = upload_request(dir.path());
        let index_lock = library.join(".git/index.lock");
        std::fs::write(&index_lock, "fixture lock").unwrap();
        assert_eq!(
            store.capture_source(&request).unwrap_err().code(),
            "library_git_error"
        );
        let journal_path = state.join("library.pending-write.json");
        let journal = std::fs::read(&journal_path).expect("durable upload decision");
        let original = library.join(UPLOAD_DIR).join("original.bin");
        let document = library.join(UPLOAD_DIR).join("source.md");
        // Reconstruct each interruption boundary from the real API's decision.
        // Recovery must not need the caller's upload to still exist.
        if phase != "document" {
            std::fs::remove_file(&document).unwrap();
        }
        if phase == "decision" {
            std::fs::remove_file(&original).unwrap();
            std::fs::remove_dir(library.join(UPLOAD_DIR)).unwrap();
        }
        std::fs::remove_file(request.upload_path.as_ref().unwrap()).unwrap();
        std::fs::remove_file(index_lock).unwrap();
        drop(store);
        let recovered = LibraryStore::open(&library, &state).expect("recover upload");
        assert_eq!(
            std::fs::read(&original).expect("replayed original"),
            [0, 255, 1, 128, 2],
            "phase {phase}"
        );
        assert_eq!(
            recovered.read_source("src-20261027-upload").unwrap().path,
            format!("{UPLOAD_DIR}/source.md")
        );
        assert_eq!(recovered.document_count().unwrap(), 1);
        assert_eq!(
            git(&library, &["ls-files"]).trim(),
            format!("{UPLOAD_DIR}/source.md")
        );
        assert!(!journal_path.exists());
        // Crash after commit but before clearing the journal: equal original
        // bytes are accepted without a replacement or a duplicate commit.
        #[cfg(unix)]
        let original_inode = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&original).unwrap().ino()
        };
        std::fs::write(&journal_path, &journal).unwrap();
        LibraryStore::open(&library, &state).expect("replay committed upload");
        assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
        assert_eq!(std::fs::read(&original).unwrap(), [0, 255, 1, 128, 2]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(&original).unwrap().ino(), original_inode);
        }
        assert!(!journal_path.exists());
    }
}

#[test]
fn upload_capture_allocates_around_occupied_paths_without_source_documents() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let request = upload_request(dir.path());
    let original = format!("{UPLOAD_DIR}/original.bin");
    write(&library, &original, "previous upload");
    std::fs::create_dir(library.join(format!("{UPLOAD_DIR}-2"))).unwrap();
    std::fs::write(library.join(format!("{UPLOAD_DIR}-3")), "occupied file").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        dir.path().join("missing"),
        library.join(format!("{UPLOAD_DIR}-4")),
    )
    .unwrap();
    let captured = store
        .capture_source(&request)
        .expect("capture around occupied entries");
    assert_eq!(
        std::fs::read(library.join(&original)).unwrap(),
        b"previous upload"
    );
    assert_eq!(
        captured.id,
        if cfg!(unix) {
            "src-20261027-upload-5"
        } else {
            "src-20261027-upload-4"
        }
    );
    assert!(!library.join(UPLOAD_DIR).join("source.md").exists());
    assert_eq!(store.document_count().unwrap(), 1);
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(library.join(format!("{UPLOAD_DIR}-4"))).unwrap(),
        dir.path().join("missing")
    );
}

#[test]
fn upload_recovery_preserves_a_conflicting_original_and_the_decision() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let request = upload_request(dir.path());
    let index_lock = library.join(".git/index.lock");
    std::fs::write(&index_lock, "fixture lock").unwrap();
    assert_eq!(
        store.capture_source(&request).unwrap_err().code(),
        "library_git_error"
    );
    let journal_path = state.join("library.pending-write.json");
    let journal = std::fs::read(&journal_path).unwrap();
    let original = library.join(UPLOAD_DIR).join("original.bin");
    std::fs::write(&original, "existing unrelated original").unwrap();
    std::fs::remove_file(library.join(UPLOAD_DIR).join("source.md")).unwrap();
    std::fs::remove_file(index_lock).unwrap();
    drop(store);
    let error = LibraryStore::open(&library, &state)
        .err()
        .expect("refuse conflict");
    assert_eq!(error.code(), "io_error");
    assert_eq!(
        std::fs::read(&original).unwrap(),
        b"existing unrelated original"
    );
    assert_eq!(std::fs::read(&journal_path).unwrap(), journal);
    assert!(!library.join(UPLOAD_DIR).join("source.md").exists());
}

#[test]
fn upload_invalid_decisions_are_preserved_without_publication() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let index_lock = library.join(".git/index.lock");
    std::fs::write(&index_lock, "fixture lock").unwrap();
    assert_eq!(
        store
            .capture_source(&upload_request(dir.path()))
            .unwrap_err()
            .code(),
        "library_git_error"
    );
    let journal_path = state.join("library.pending-write.json");
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    std::fs::remove_file(library.join(UPLOAD_DIR).join("source.md")).unwrap();
    std::fs::remove_file(library.join(UPLOAD_DIR).join("original.bin")).unwrap();
    std::fs::remove_file(index_lock).unwrap();
    drop(store);
    for invalid in [
        "older_version",
        "newer_version",
        "missing_original",
        "settings",
        "different_parent",
        "not_original",
        "wrong_bytes",
    ] {
        let mut decision = journal.clone();
        match invalid {
            "older_version" => decision["version"] = 2.into(),
            "newer_version" => decision["version"] = 4.into(),
            "missing_original" => {
                decision.as_object_mut().unwrap().remove("original");
            }
            "settings" => decision["kind"] = "settings".into(),
            "different_parent" => {
                decision["original"]["relative"] = "knowledge/original.bin".into()
            }
            "not_original" => {
                decision["original"]["relative"] = format!("{UPLOAD_DIR}/other.bin").into()
            }
            "wrong_bytes" => decision["original"]["bytes"] = serde_json::json!([7, 8]),
            _ => unreachable!("test case"),
        }
        let bytes = serde_json::to_vec(&decision).unwrap();
        std::fs::write(&journal_path, &bytes).unwrap();
        let error = LibraryStore::open(&library, &state)
            .err()
            .expect("refuse invalid upload decision");
        assert_eq!(error.code(), "library_journal_invalid", "case {invalid}");
        assert_eq!(std::fs::read(&journal_path).unwrap(), bytes);
        assert!(!library.join(UPLOAD_DIR).join("source.md").exists());
        assert!(
            std::fs::read_dir(library.join(UPLOAD_DIR))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(!library.join("knowledge/original.bin").exists());
    }
}

#[test]
fn legacy_document_decisions_remain_replayable() {
    for version in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let state = dir.path().join("state");
        initialize(&library);
        LibraryStore::open(&library, &state).unwrap();
        let journal = serde_json::json!({
            "version": version, "kind": "document", "root": library.canonicalize().unwrap(),
            "relative": "knowledge/domain/recovered.md", "text": DOMAIN, "token": "123-456"
        });
        std::fs::write(
            state.join("library.pending-write.json"),
            journal.to_string(),
        )
        .unwrap();
        let recovered = LibraryStore::open(&library, &state).expect("recover legacy document");
        assert_eq!(
            recovered
                .read_text(Path::new("knowledge/domain/recovered.md"))
                .unwrap(),
            DOMAIN
        );
        assert_eq!(recovered.document_count().unwrap(), 1);
    }
}

#[test]
fn settings_decision_recovers_without_indexing_and_replay_does_not_duplicate_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let journal_path = state.join("library.pending-write.json");
    let journal = serde_json::json!({
        "version": 2, "kind": "settings", "root": library.canonicalize().unwrap(),
        "relative": ".office/config.yaml", "text": "retention:\n  meeting: 30\n", "token": "123-456"
    });
    std::fs::write(&journal_path, journal.to_string()).unwrap();
    assert_eq!(
        store.settings().unwrap_err().code(),
        "library_write_pending"
    );
    // The decided write, not a valid old config, is the recovery precondition.
    std::fs::remove_file(library.join(".office/config.yaml")).unwrap();
    let recovered = LibraryStore::open(&library, &state).expect("recover settings");
    assert_eq!(recovered.settings().unwrap().retention["meeting"], Some(30));
    assert_eq!(recovered.document_count().unwrap(), 0);
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        git(&library, &["show", "HEAD:.office/config.yaml"]),
        "retention:\n  meeting: 30\n"
    );
    assert!(!journal_path.exists());
    std::fs::write(&journal_path, journal.to_string()).unwrap();
    let replayed = LibraryStore::open(&library, &state).expect("post-commit replay");
    assert_eq!(replayed.settings().unwrap().retention["meeting"], Some(30));
    assert_eq!(replayed.document_count().unwrap(), 0);
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert!(!journal_path.exists());
}

#[test]
fn invalid_settings_decisions_are_preserved_and_never_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    LibraryStore::open(&library, &state).expect("store");
    for (version, kind, relative, text, code) in [
        (
            2,
            "settings",
            "knowledge/domain/escape.md",
            "{}",
            "library_journal_invalid",
        ),
        (
            2,
            "settings",
            ".office/config.yaml",
            "retention:\n  meeting: 0",
            "invalid_library_setting",
        ),
        (
            2,
            "unknown",
            ".office/config.yaml",
            "{}",
            "library_journal_invalid",
        ),
        (
            3,
            "settings",
            ".office/config.yaml",
            "{}",
            "library_journal_invalid",
        ),
        (
            1,
            "settings",
            ".office/config.yaml",
            "{}",
            "library_journal_invalid",
        ),
    ] {
        let journal = serde_json::json!({"version": version, "kind": kind,
            "root": library.canonicalize().unwrap(), "relative": relative,
            "text": text, "token": "123-456"})
        .to_string();
        let path = state.join("library.pending-write.json");
        std::fs::write(&path, &journal).unwrap();
        let error = LibraryStore::open(&library, &state)
            .err()
            .expect("refuse journal");
        assert_eq!(error.code(), code);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), journal);
        assert_eq!(
            std::fs::read_to_string(library.join(".office/config.yaml")).unwrap(),
            ""
        );
        assert!(!library.join("knowledge/domain/escape.md").exists());
    }
}

#[test]
fn settings_updates_read_the_latest_config_and_do_not_lose_other_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let a = LibraryStore::open(&library, &state).expect("store a");
    let b = LibraryStore::open(&library, &state).expect("store b");
    a.set_setting("retention.meeting", serde_json::json!(10))
        .unwrap();
    b.set_setting("sensitivity.note", serde_json::json!("private"))
        .unwrap();
    a.set_setting("packs", serde_json::json!(["receipts"]))
        .unwrap();
    let settings = b.settings().unwrap();
    assert_eq!(settings.retention["meeting"], Some(10));
    assert_eq!(settings.sensitivity["note"].as_str(), "private");
    assert_eq!(settings.packs, ["receipts"]);
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "3");
    assert_eq!(a.document_count().unwrap(), 0);
}

#[test]
fn settings_commit_failure_leaves_a_recoverable_decision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let index_lock = library.join(".git/index.lock");
    std::fs::write(&index_lock, "fixture lock").unwrap();
    assert_eq!(
        store
            .set_setting("retention.meeting", serde_json::json!(20))
            .unwrap_err()
            .code(),
        "library_git_error"
    );
    assert!(state.join("library.pending-write.json").exists());
    // Opening recovers under the bounded writer lock. Read refusal for an
    // abandoned settings journal is covered by the explicit journal fixture;
    // a read while a kernel writer lock is still live may observe new config.
    std::fs::remove_file(index_lock).unwrap();
    let recovered = LibraryStore::open(&library, &state).expect("finish config commit");
    assert_eq!(recovered.settings().unwrap().retention["meeting"], Some(20));
    assert_eq!(recovered.document_count().unwrap(), 0);
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        git(&library, &["show", "HEAD:.office/config.yaml"]),
        std::fs::read_to_string(library.join(".office/config.yaml")).unwrap()
    );
    assert!(!state.join("library.pending-write.json").exists());
}

#[cfg(unix)]
#[test]
fn settings_updates_refuse_a_symlink_without_changing_its_target() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let outside = dir.path().join("outside.yaml");
    std::fs::write(&outside, "packs: [receipts]\n").unwrap();
    std::fs::remove_file(library.join(".office/config.yaml")).unwrap();
    symlink(&outside, library.join(".office/config.yaml")).unwrap();
    assert_eq!(
        store
            .set_setting("retention.meeting", serde_json::json!(10))
            .unwrap_err()
            .code(),
        "unsafe_path"
    );
    assert_eq!(
        std::fs::read_to_string(outside).unwrap(),
        "packs: [receipts]\n"
    );
    assert!(!state.join("library.pending-write.json").exists());
}

#[test]
fn concurrent_writes_have_separate_commits_and_consistent_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let a = LibraryStore::open(&library, &state).expect("store a");
    let b = LibraryStore::open(&library, &state).expect("store b");
    let start = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            start.wait();
            a.write_document(
                Path::new("knowledge/domain/one.md"),
                "---\nid: dom-one\n---\nconcurrency one",
            )
            .expect("write one")
        });
        let two = scope.spawn(|| {
            start.wait();
            b.write_document(
                Path::new("knowledge/domain/two.md"),
                "---\nid: dom-two\n---\nconcurrency two",
            )
            .expect("write two")
        });
        one.join().expect("one");
        two.join().expect("two");
    });
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(a.document_count().expect("count"), 2);
    for name in ["one", "two"] {
        let file = a
            .read_document(Path::new(&format!("knowledge/domain/{name}.md")))
            .expect("read");
        assert_eq!(a.get(&format!("dom-{name}")).expect("get"), Some(file));
    }
    assert_eq!(
        a.search("concurrency", &SearchFilter::default())
            .expect("search")
            .len(),
        2
    );
}

#[test]
fn writes_commit_fixed_author_and_never_stage_originals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    write(
        &library,
        "sources/2026/10/meeting/original.txt",
        "private raw text",
    );
    let store = LibraryStore::open(&library, &dir.path().join("state")).expect("open");
    store
        .write_document(Path::new("sources/2026/10/meeting/source.md"), MEETING)
        .expect("write");
    assert_eq!(
        git(&library, &["log", "-1", "--format=%an <%ae>"]).trim(),
        "orbit-office <office@orbit.local>"
    );
    assert_eq!(
        git(&library, &["ls-files"]).trim(),
        "sources/2026/10/meeting/source.md"
    );
    assert!(git(&library, &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn port_reads_and_search_succeed_while_writer_lock_is_held() {
    use office_core::ports::Library;
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let port: &dyn Library = &store;
    let path = Path::new("knowledge/domain/dom-acme.md");
    port.write_document(path, DOMAIN).expect("port write");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("library.writer.lock"))
        .expect("lock file");
    file.lock().expect("hold writer lock");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let port: &dyn Library = &store;
            let record = port.read_document(path).expect("read under lock");
            assert_eq!(record.id, "dom-acme-renewal");
            assert_eq!(
                port.search("renewal", &SearchFilter::default())
                    .expect("search under lock")
                    .len(),
                1
            );
            assert_eq!(port.history(path).expect("history under lock").len(), 1);
            assert!(port.is_initialized().expect("initialized"));
            port.settings().expect("settings under lock");
            sender.send(()).expect("read done");
        });
        let outcome = receiver.recv_timeout(std::time::Duration::from_secs(2));
        // Always release before joining, including on a failed deadline assertion.
        drop(file);
        reader.join().expect("reader");
        outcome.expect("reads must finish before writer lock is released");
    });
}

#[test]
#[cfg(unix)]
fn reads_succeed_during_a_journaled_write() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::time::{Duration, Instant};

    // The explicit test-only subprocess isolates PATH from every other test.
    // The git shim uses this same fixture as a bounded commit barrier, never
    // a production self-reexec path. EOF releases it even if the reader panics.
    if let Some(marker) = std::env::var_os("OFFICE_TEST_COMMIT_BARRIER") {
        let address = std::fs::read_to_string(marker).expect("barrier address");
        let mut stream = TcpStream::connect(address).expect("connect commit barrier");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("barrier deadline");
        let mut release = [0];
        stream.read_exact(&mut release).expect("release commit");
        return;
    }
    let Some(root) = std::env::var_os("OFFICE_TEST_LIVE_WRITE_ROOT") else {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).expect("shim directory");
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let real_git = std::env::split_paths(&inherited_path)
            .map(|path| path.join("git"))
            .find(|path| path.is_file())
            .expect("git on PATH")
            .canonicalize()
            .expect("resolve git");
        symlink(real_git, bin.join("git.real")).expect("real git");
        let shim = bin.join("git");
        std::fs::write(
            &shim,
            "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = commit ] && [ -f \"$HOME/commit-barrier\" ]; then\n\
                 OFFICE_TEST_COMMIT_BARRIER=\"$HOME/commit-barrier\" \"$HOME/test-harness\" --exact reads_succeed_during_a_journaled_write --nocapture || exit $?\n\
               fi\n\
             done\n\
             exec \"${0%/*}/git.real\" \"$@\"\n",
        )
        .expect("git shim");
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700))
            .expect("executable shim");
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&inherited_path)),
        )
        .expect("isolated PATH");
        let mut command =
            std::process::Command::new(std::env::current_exe().expect("test fixture"));
        command
            .args(["--exact", "reads_succeed_during_a_journaled_write"])
            .current_dir(dir.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", dir.path())
            .env("OFFICE_TEST_LIVE_WRITE_ROOT", dir.path());
        let (status, output) =
            office_common::process::run(&mut command, dir.path()).expect("isolated fixture");
        assert!(
            status.success(),
            "live-write fixture exited {status}: {output}"
        );
        return;
    };
    let root = Path::new(&root);
    let library = root.join("library");
    let state = root.join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let path = Path::new("knowledge/domain/dom-acme.md");
    store.write_document(path, DOMAIN).expect("initial write");
    let previous_history = store.history(path).expect("initial history");
    let updated = DOMAIN.replace("title: Acme renewal", "title: Acme renewal updated");
    let listener = TcpListener::bind("127.0.0.1:0").expect("commit barrier");
    listener.set_nonblocking(true).expect("nonblocking accept");
    symlink(
        std::env::current_exe().expect("test fixture"),
        state.join("test-harness"),
    )
    .expect("barrier fixture");
    std::fs::write(
        state.join("commit-barrier"),
        listener.local_addr().expect("barrier address").to_string(),
    )
    .expect("enable commit barrier");

    std::thread::scope(|scope| {
        let writer = scope.spawn(|| store.write_document(path, &updated));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut barrier = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(!writer.is_finished(), "writer exited before commit barrier");
                    assert!(
                        Instant::now() < deadline,
                        "writer did not reach commit barrier"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept commit barrier: {error}"),
            }
        };
        assert!(state.join("library.pending-write.json").exists());
        let document = store.read_document(path);
        let indexed = store.get("dom-acme-renewal");
        let hits = store.search("renewal", &SearchFilter::default());
        let history = store.history(path);
        let settings = store.settings();
        let count = store.document_count();
        let hash = store.file_sha256(path);
        let health = store.health();
        let still_in_flight =
            !writer.is_finished() && state.join("library.pending-write.json").exists();
        barrier.write_all(&[1]).expect("release commit");
        drop(barrier);
        let written = writer.join().expect("writer").expect("completed write");

        assert!(
            still_in_flight,
            "reads must complete before the writer is released"
        );
        assert_eq!(document.expect("read during write"), written);
        assert_eq!(indexed.expect("get during write"), Some(written.clone()));
        let hits = hits.expect("search during write");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Acme renewal updated");
        assert_eq!(history.expect("history during write"), previous_history);
        settings.expect("settings during write");
        assert_eq!(count.expect("count during write"), 1);
        assert_eq!(hash.expect("hash during write").len(), 64);
        assert_eq!(health.expect("health during write").documents_total, 1);
        assert_eq!(store.read_document(path).expect("completed read"), written);
        assert_eq!(store.history(path).expect("completed history").len(), 2);
        assert!(!state.join("library.pending-write.json").exists());
    });
}

#[test]
fn readiness_refuses_uninitialized_and_validates_settings_on_every_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let store = LibraryStore::open(&library, &dir.path().join("state")).expect("store");
    assert!(!store.is_initialized().expect("not initialized"));
    let path = Path::new("knowledge/domain/example.md");
    assert_eq!(
        store.write_document(path, "example").unwrap_err().code(),
        "library_not_initialized"
    );
    assert!(!library.join(path).exists());
    initialize(&library);
    assert!(store.is_initialized().expect("initialized"));
    assert_eq!(
        store.settings().expect("defaults"),
        office_core::domain::settings::Settings::default()
    );
    std::fs::write(
        library.join(".office/config.yaml"),
        "retention:\n  meeting: -1\n",
    )
    .expect("invalid config");
    let error = office_core::ports::Library::settings(&store).unwrap_err();
    let office_core::Error::InvalidSetting { key, .. } = error else {
        panic!("expected typed setting error, got {error:?}")
    };
    assert_eq!(key, "retention.meeting");
}

#[test]
fn writes_preserve_unrelated_staging_and_identical_writes_have_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    write(&library, "unrelated.md", "user staged content");
    git(&library, &["add", "unrelated.md"]);
    let store = LibraryStore::open(&library, &dir.path().join("state")).expect("store");
    let path = Path::new("knowledge/domain/dom-acme.md");
    store.write_document(path, DOMAIN).expect("write");
    store
        .write_document(path, DOMAIN)
        .expect("identical write still commits");
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(
        git(&library, &["ls-tree", "--name-only", "-r", "HEAD"]).trim(),
        "knowledge/domain/dom-acme.md"
    );
    assert_eq!(
        git(&library, &["diff", "--cached", "--name-only"]).trim(),
        "unrelated.md"
    );
    // Empty commits are real write events, even though path-limited git history
    // only contains the commit that changed this document's content.
    let history = store.history(path).expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].author, "orbit-office");
    assert_eq!(history[0].email, "office@orbit.local");
    assert!(history[0].authored_at.contains('T'));
}

#[test]
fn interrupted_write_replays_and_committed_replay_does_not_duplicate_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let root = library.canonicalize().expect("root");
    let path = "knowledge/domain/recovered.md";
    let journal = serde_json::json!({"version": 1, "root": root, "relative": path, "text": DOMAIN, "token": "123-456"});
    std::fs::write(
        state.join("library.pending-write.json"),
        journal.to_string(),
    )
    .expect("interrupted decision");
    let assert_pending_reads = || {
        for error in [
            store.read_document(Path::new(path)).unwrap_err(),
            store.get("dom-acme-renewal").unwrap_err(),
            store
                .search("renewal", &SearchFilter::default())
                .unwrap_err(),
            store.history(Path::new(path)).unwrap_err(),
            store.settings().unwrap_err(),
            store.document_count().unwrap_err(),
            store.file_sha256(Path::new(path)).unwrap_err(),
            store.health().unwrap_err(),
        ] {
            assert_eq!(error.code(), "library_write_pending");
        }
        assert_eq!(
            std::fs::read_to_string(state.join("library.pending-write.json"))
                .expect("kept journal"),
            journal.to_string()
        );
    };
    assert!(!state.join("library.writer.lock").exists());
    assert_pending_reads();
    assert!(!state.join("library.writer.lock").exists());
    let recovered = LibraryStore::open(&library, &state).expect("recover before exposure");
    let doc = recovered
        .read_document(Path::new(path))
        .expect("replayed file");
    assert_eq!(recovered.get(&doc.id).expect("index"), Some(doc));
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    // Model a crash after commit but before the completion record was cleared.
    std::fs::write(
        state.join("library.pending-write.json"),
        journal.to_string(),
    )
    .expect("post-commit decision");
    // A stale holder record and an existing, unlocked file are not a live writer.
    assert!(state.join("library.writer.lock").exists());
    assert_pending_reads();
    LibraryStore::open(&library, &state).expect("idempotent recovery");
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert!(!state.join("library.pending-write.json").exists());
}

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

#[test]
fn index_tracks_replaced_identity_and_refuses_duplicate_id_before_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    let state = dir.path().join("state");
    let store = LibraryStore::open(&library, &state).expect("store");
    let path = Path::new("knowledge/domain/identity.md");
    store
        .write_document(path, "---\nid: dom-before\n---\nbefore")
        .expect("before");
    store
        .write_document(path, "---\nid: dom-after\n---\nafter")
        .expect("after");
    assert!(store.get("dom-before").expect("old id").is_none());
    assert_eq!(
        store.get("dom-after").expect("new id"),
        Some(store.read_document(path).expect("file"))
    );
    let duplicate = Path::new("knowledge/domain/duplicate.md");
    assert_eq!(
        store
            .write_document(duplicate, "---\nid: dom-after\n---\nduplicate")
            .unwrap_err()
            .code(),
        "library_document_conflict"
    );
    assert!(!library.join(duplicate).exists());
    assert!(!state.join("library.pending-write.json").exists());
    assert_eq!(store.document_count().expect("count"), 1);
}

#[test]
fn writer_waits_for_the_state_lock_before_document_index_and_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("store");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(state.join("library.writer.lock"))
        .expect("lock file");
    lock.lock().expect("hold lock");
    let (started, starting) = std::sync::mpsc::sync_channel(1);
    let (done, result) = std::sync::mpsc::sync_channel(1);
    let relative = Path::new("knowledge/domain/waits.md");
    std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            started.send(()).expect("started");
            let outcome = store.write_document(relative, DOMAIN);
            done.send(outcome).expect("done");
        });
        starting
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("writer started");
        let while_locked = result.recv_timeout(std::time::Duration::from_millis(100));
        let file_exists_while_locked = library.join(relative).exists();
        drop(lock);
        writer.join().expect("writer");
        assert!(matches!(
            while_locked,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(!file_exists_while_locked);
        result
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("writer finished")
            .expect("successful write");
    });
    assert_eq!(store.document_count().expect("index"), 1);
    assert_eq!(store.history(relative).expect("commit").len(), 1);
}

#[test]
fn written_documents_survive_reindex_and_unscanned_paths_are_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).expect("open store");

    // Paths `scan_documents` skips. On the base commit write_document indexes
    // them and the next reindex deletes those rows.
    let refused = [
        ("proposals/p.md", "prop-1"),
        ("notes.md", "note-1"),
        ("sources/src-1/v2.md", "src-v2"),
        ("artifacts/2026/10/art-1/report.md", "art-report"),
    ];
    for (relative, id) in refused {
        let path = Path::new(relative);
        let error = store
            .write_document(path, &format!("---\nid: {id}\n---\ndroppedtoken"))
            .unwrap_err();
        assert_eq!(error.code(), "unsafe_path", "{relative}");
        assert!(!library.join(path).exists(), "{relative} was written");
        assert!(store.get(id).expect("get").is_none(), "{id}");
    }
    assert!(!state.join("library.pending-write.json").exists());
    assert_eq!(store.document_count().expect("count"), 0);
    assert!(
        store
            .search("droppedtoken", &SearchFilter::default())
            .expect("search")
            .is_empty()
    );

    let accepted = [
        (
            "sources/2026/10/src-1/source.md",
            "src-1",
            DocumentKind::Source,
            "sourcetoken",
            SOURCE_ONE,
        ),
        (
            "knowledge/domain/dom-a.md",
            "dom-a",
            DocumentKind::Domain,
            "domaintoken",
            DOMAIN_ONE,
        ),
        (
            "knowledge/runbooks/rb-a.md",
            "rb-a",
            DocumentKind::Runbook,
            "runbooktoken",
            RUNBOOK_ONE,
        ),
        (
            "artifacts/2026/10/art-1/artifact.md",
            "art-1",
            DocumentKind::Artifact,
            "artifacttoken",
            ARTIFACT_ONE,
        ),
    ];
    for (relative, id, kind, token, text) in accepted {
        let record = store
            .write_document(Path::new(relative), text)
            .expect(relative);
        assert_eq!(record.id, id);
        assert_eq!(record.kind, kind);
        assert!(record.body.contains(token), "{id}");
    }

    let ids = ["src-1", "dom-a", "rb-a", "art-1"];
    let gets_before: Vec<_> = ids
        .iter()
        .map(|id| store.get(id).expect("get").expect("present"))
        .collect();
    let search_before = search_sorted(&store, "keptafterreindex");
    assert_eq!(search_before.len(), accepted.len());
    let tokens = [
        ("src-1", "sourcetoken"),
        ("dom-a", "domaintoken"),
        ("rb-a", "runbooktoken"),
        ("art-1", "artifacttoken"),
    ];
    let token_hits_before: Vec<_> = tokens
        .iter()
        .map(|(id, token)| token_hit(&store, id, token))
        .collect();
    let source_hits_before = search_sorted_kind(&store, "keptafterreindex", DocumentKind::Source);
    assert_eq!(source_hits_before.len(), 1);
    assert_eq!(source_hits_before[0].id, "src-1");

    let indexed = store.reindex().expect("reindex");
    assert_eq!(indexed, u32::try_from(accepted.len()).expect("count"));
    assert_eq!(store.document_count().expect("count"), indexed);

    let gets_after: Vec<_> = ids
        .iter()
        .map(|id| store.get(id).expect("get").expect("present"))
        .collect();
    assert_eq!(gets_before, gets_after);
    assert_eq!(search_before, search_sorted(&store, "keptafterreindex"));
    let token_hits_after: Vec<_> = tokens
        .iter()
        .map(|(id, token)| token_hit(&store, id, token))
        .collect();
    assert_eq!(token_hits_before, token_hits_after);
    assert_eq!(
        source_hits_before,
        search_sorted_kind(&store, "keptafterreindex", DocumentKind::Source)
    );
    for id in ["prop-1", "note-1", "src-v2", "art-report"] {
        assert!(store.get(id).expect("get").is_none(), "{id}");
    }
}

fn search_sorted(store: &LibraryStore, query: &str) -> Vec<SearchHit> {
    let mut hits = store
        .search(query, &SearchFilter::default())
        .expect("search");
    hits.sort_by(|left, right| left.id.cmp(&right.id));
    hits
}

fn search_sorted_kind(store: &LibraryStore, query: &str, kind: DocumentKind) -> Vec<SearchHit> {
    let mut hits = store
        .search(
            query,
            &SearchFilter {
                kind: Some(kind),
                ..SearchFilter::default()
            },
        )
        .expect("search");
    hits.sort_by(|left, right| left.id.cmp(&right.id));
    hits
}

fn token_hit(store: &LibraryStore, id: &str, token: &str) -> SearchHit {
    let hits = search_sorted(store, token);
    assert_eq!(hits.len(), 1, "{id}");
    assert_eq!(hits[0].id, id);
    hits.into_iter().next().expect("one hit")
}

const SOURCE_ONE: &str = "\
---
id: src-1
kind: note
title: Source one
---

sourcetoken keptafterreindex
";

const DOMAIN_ONE: &str = "\
---
id: dom-a
type: domain
title: Domain one
summary: A page that must survive reindex.
---

domaintoken keptafterreindex
";

const RUNBOOK_ONE: &str = "\
---
id: rb-a
type: runbook
title: Runbook one
---

runbooktoken keptafterreindex
";

const ARTIFACT_ONE: &str = "\
---
id: art-1
kind: report
title: Artifact one
series: renewal-research
---

artifacttoken keptafterreindex
";

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

#[test]
fn init_copies_missing_templates_and_health_reports_template_drift() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let library = workspace.join("library");
    let state = dir.path().join("state");
    let first =
        LibraryStore::initialize_at(&library, &state, None, &workspace).expect("fresh init");
    assert!(!first.already_initialized);
    for bundled in office_core::domain::template::BUNDLED {
        let relative = format!(".office/templates/{}", bundled.filename);
        assert_eq!(
            std::fs::read(library.join(&relative)).expect("template bytes"),
            bundled.markdown.as_bytes(),
            "{}",
            bundled.filename
        );
        assert!(first.created.iter().any(|path| path == &relative));
    }
    assert!(markdown_under(&library.join("knowledge")).is_empty());
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");

    let user = library.join(".office/templates/user.md");
    let person = library.join(".office/templates/person.md");
    let original_person = std::fs::read(&person).expect("person bytes");
    let mut edited = std::fs::read(&user).expect("user bytes");
    edited.extend_from_slice(b"\nEdited by the user.\n");
    std::fs::write(&user, &edited).expect("edit template");
    std::fs::remove_file(&person).expect("delete template");
    let second = LibraryStore::initialize_at(&library, &state, None, &workspace).expect("restore");
    assert!(!second.already_initialized);
    assert_eq!(
        second.created,
        vec![".office/templates/person.md".to_string()]
    );
    assert_eq!(std::fs::read(&user).expect("edited bytes"), edited);
    assert_eq!(
        std::fs::read(&person).expect("restored bytes"),
        original_person
    );
    // The deleted file matched HEAD, so restoring it does not add a commit,
    // and the edit to another template stays unstaged.
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    let dirty = git(&library, &["status", "--porcelain"]);
    assert!(
        dirty
            .lines()
            .any(|line| line.ends_with(".office/templates/user.md")),
        "{dirty}"
    );
    assert!(
        !dirty.lines().any(|line| line.contains("person.md")),
        "{dirty}"
    );

    std::fs::remove_file(&person).expect("delete committed template");
    git(&library, &["add", "--", ".office/templates/person.md"]);
    git(
        &library,
        &[
            "commit",
            "--quiet",
            "-m",
            "remove person template",
            "--",
            ".office/templates/person.md",
        ],
    );
    let restored =
        LibraryStore::initialize_at(&library, &state, None, &workspace).expect("committed restore");
    assert!(!restored.already_initialized);
    assert_eq!(
        restored.created,
        vec![".office/templates/person.md".to_string()]
    );
    assert_eq!(std::fs::read(&user).expect("edited bytes"), edited);
    assert_eq!(
        std::fs::read(&person).expect("restored bytes"),
        original_person
    );
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "3");
    assert_eq!(
        git(
            &library,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        )
        .trim(),
        ".office/templates/person.md"
    );
    let still_dirty = git(&library, &["status", "--porcelain"]);
    assert!(
        still_dirty
            .lines()
            .any(|line| line.ends_with(".office/templates/user.md")),
        "{still_dirty}"
    );
    assert!(
        !still_dirty.lines().any(|line| line.contains("person.md")),
        "{still_dirty}"
    );
    let third = LibraryStore::initialize_at(&library, &state, None, &workspace).expect("complete");
    assert!(third.already_initialized);
    assert!(third.created.is_empty());
    assert_eq!(std::fs::read(&user).expect("edit survives"), edited);

    std::fs::write(
        library.join("knowledge/domain/ada.md"),
        "---\nid: dom-person-ada\ntype: domain\ntemplate: person\ntitle: Ada\n---\n\n## Role\nKeeps the notes.\n",
    )
    .expect("meeting page");
    std::fs::write(
        library.join("knowledge/domain/bev.md"),
        "---\nid: dom-person-bev\ntype: domain\ntemplate: person\ntitle: Bev\n---\n\n## History\nNo role.\n",
    )
    .expect("off-template page");
    std::fs::write(
        library.join("knowledge/domain/free.md"),
        "---\nid: dom-free\ntype: domain\ntitle: Free\n---\n\n## Role\nUnbound.\n",
    )
    .expect("free-form page");
    std::fs::write(
        library.join(".office/templates/notes.md"),
        "not a template\n",
    )
    .expect("broken template");
    let store = LibraryStore::open(&library, &state).expect("open");
    assert_eq!(store.reindex().expect("reindex"), 3);
    let health = store.health().expect("status still returns");
    assert!(health.initialized);
    assert_eq!(health.documents_total, 3);
    assert_eq!(health.domain, 3);
    assert_eq!(health.pages_off_template, 1);
    assert_eq!(
        health.template_invalid,
        vec![".office/templates/notes.md".to_string()]
    );
}

#[cfg(unix)]
fn library_with_links_outside(link_knowledge_file: bool) -> (tempfile::TempDir, LibraryStore) {
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
    write(
        &library,
        "sources/2026/10/src-inside/source.md",
        "---\nid: src-inside\nkind: note\ntitle: Inside source\n---\n\ninside library source\n",
    );
    let outside = dir.path().join("outside");
    write(
        &outside,
        "secret.md",
        "---\nid: dom-outside-file\ntype: domain\ntitle: Outside file\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "tree/page.md",
        "---\nid: dom-outside-dir\ntype: domain\ntitle: Outside dir\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "source.md",
        "---\nid: src-outside-file\nkind: note\ntitle: Outside source file\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "source-tree/source.md",
        "---\nid: src-outside-dir\nkind: note\ntitle: Outside source dir\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "artifact-tree/artifact.md",
        "---\nid: art-outside\nkind: doc\ntitle: Outside artifact\n---\n\nOUTSIDE library secret\n",
    );
    let store = LibraryStore::open(&library, &state).expect("open");
    if link_knowledge_file {
        symlink(
            outside.join("secret.md"),
            library.join("knowledge/domain/linked.md"),
        )
        .expect("knowledge file symlink");
    }
    symlink(outside.join("tree"), library.join("knowledge/outside"))
        .expect("knowledge dir symlink");
    std::fs::create_dir_all(library.join("sources/2026/10/src-linked")).expect("linked source dir");
    symlink(
        outside.join("source.md"),
        library.join("sources/2026/10/src-linked/source.md"),
    )
    .expect("source file symlink");
    symlink(outside.join("source-tree"), library.join("sources/linked"))
        .expect("sources dir symlink");
    let artifacts = library.join("artifacts");
    std::fs::remove_dir_all(&artifacts).expect("remove real artifacts layer");
    symlink(outside.join("artifact-tree"), &artifacts).expect("artifacts layer symlink");
    (dir, store)
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

/// A missing indexed layer is an empty part of a complete scan. A regular
/// file where a layer directory belongs is an enumeration failure: `reindex`
/// returns `io_error` and does not rebuild, so rows indexed earlier stay
/// searchable. Replacing the directory with a file yields `ENOTDIR` without
/// depending on permissions, including when the test runs as root.
#[test]
fn enumeration_error_aborts_reindex_and_preserves_the_prior_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "sources/2026/10/src-kept/source.md",
        "---\nid: src-kept\nkind: note\ntitle: Kept source\n---\n\nkeptsource token\n",
    );
    write(
        &library,
        "knowledge/domain/dom-kept.md",
        "---\nid: dom-kept\ntype: domain\ntitle: Kept page\n---\n\nkeptpage token\n",
    );
    let store = LibraryStore::open(&library, &state).expect("open");
    let root = library.canonicalize().expect("canonical library");
    assert_eq!(store.reindex().expect("initial reindex"), 2);
    let indexed_before = search_sorted(&store, "kept");
    assert_eq!(indexed_before.len(), 2);
    assert_eq!(store.document_count().expect("count"), 2);

    std::fs::remove_dir(root.join("artifacts")).expect("remove empty artifacts layer");
    assert!(
        !root.join("artifacts").exists(),
        "artifacts layer is absent"
    );
    assert_eq!(store.reindex().expect("absent layer reindexes"), 2);
    assert_eq!(search_sorted(&store, "kept"), indexed_before);
    assert_eq!(
        store
            .get("src-kept")
            .expect("get source")
            .expect("source")
            .id,
        "src-kept"
    );
    assert_eq!(
        store.get("dom-kept").expect("get page").expect("page").id,
        "dom-kept"
    );

    std::fs::remove_dir_all(root.join("knowledge")).expect("remove knowledge layer");
    std::fs::write(root.join("knowledge"), "not a directory\n").expect("knowledge file");
    assert!(root.join("knowledge").is_file(), "layer path is a file");
    assert!(
        root.join("sources/2026/10/src-kept/source.md").is_file(),
        "source remains on disk"
    );

    let error = store
        .reindex()
        .expect_err("enumeration failure must abort reindex");
    assert_eq!(error.code(), "io_error");
    let Error::Io {
        action,
        path,
        source,
    } = &error
    else {
        panic!("expected Io, got {error:?}");
    };
    assert_eq!(*action, "list Library directory");
    assert_eq!(path, &root.join("knowledge"));
    assert_eq!(source.kind(), std::io::ErrorKind::NotADirectory);

    assert_eq!(store.document_count().expect("count unchanged"), 2);
    assert_eq!(search_sorted(&store, "kept"), indexed_before);
    token_hit(&store, "src-kept", "keptsource");
    token_hit(&store, "dom-kept", "keptpage");
    assert!(store.get("dom-kept").expect("get").is_some());
    let pages = store
        .knowledge_pages()
        .expect_err("knowledge walk fails closed");
    assert_eq!(pages.code(), "io_error");
    let Error::Io { path, .. } = &pages else {
        panic!("expected Io, got {pages:?}");
    };
    assert_eq!(path, &root.join("knowledge"));
}

fn markdown_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(
                    path.strip_prefix(root)
                        .expect("relative")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    found.sort();
    found
}

#[test]
fn attach_reads_the_head_commit_and_stages_copies_outside_the_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    initialize(&library);
    let store = LibraryStore::open(&library, &dir.path().join("state")).expect("store");
    assert_eq!(store.head_commit().expect("head"), None);
    let path = Path::new("knowledge/domain/dom-acme.md");
    store.write_document(path, DOMAIN).expect("write");
    let head = store.head_commit().expect("head").expect("a commit");
    assert_eq!(head, git(&library, &["rev-parse", "HEAD"]).trim());
    assert_eq!(store.read_text(path).expect("text"), DOMAIN);

    let staged = store
        .stage_attachment("knowledge/domain/dom-acme.md", "model view")
        .expect("stage");
    assert!(staged.starts_with(library.canonicalize().expect("root").join(".office/attach")));
    assert_eq!(
        std::fs::read_to_string(&staged).expect("staged"),
        "model view"
    );
    // A staged copy is neither indexed nor committed by the next write.
    assert_eq!(store.reindex().expect("reindex"), 1);
    store.write_document(path, DOMAIN).expect("rewrite");
    assert_eq!(
        git(&library, &["ls-tree", "--name-only", "-r", "HEAD"]).trim(),
        "knowledge/domain/dom-acme.md"
    );
    store.remove_staged(&staged).expect("remove");
    assert!(!staged.exists());
    let attach_dir = library.join(".office/attach");
    assert_eq!(std::fs::read_dir(&attach_dir).expect("dir").count(), 0);

    // Only staged files may be removed, and names stay inside the Library.
    let config = library
        .canonicalize()
        .expect("root")
        .join(".office/config.yaml");
    assert!(store.remove_staged(&config).is_err());
    assert!(config.exists());
    assert!(store.stage_attachment("../escape.md", "x").is_err());
}

#[test]
fn a_version_one_index_migrates_and_open_fills_template_and_date() {
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(&library, "knowledge/domain/dom-acme-renewal.md", DOMAIN);
    write(
        &library,
        "knowledge/domain/dom-person-jane.md",
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\nupdated_at: 2026-10-20T10:00:00Z\n---\nRenewal owner.\n",
    );
    std::fs::create_dir_all(&state).expect("state");
    {
        let conn = rusqlite::Connection::open(state.join("index.sqlite")).expect("v1 index");
        conn.execute_batch(
            "CREATE TABLE documents (id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
                 summary TEXT NOT NULL, tags TEXT NOT NULL, series TEXT, sensitivity TEXT NOT NULL,
                 path TEXT NOT NULL UNIQUE, body TEXT NOT NULL);
             CREATE INDEX documents_kind ON documents(kind);
             CREATE INDEX documents_series ON documents(series);
             CREATE TABLE cites (doc_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 target TEXT NOT NULL, PRIMARY KEY (doc_id, target));
             CREATE TABLE links (from_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 to_id TEXT NOT NULL, PRIMARY KEY (from_id, to_id));
             CREATE VIRTUAL TABLE documents_fts USING fts5(id UNINDEXED, title, summary, body, tags);
             PRAGMA user_version = 1;",
        )
        .expect("v1 schema");
    }
    // Opening must backfill without an explicit reindex: the pre-upgrade
    // rows were indexed under schema 1, so their template/date are NULL
    // until the files are rescanned.
    let store = LibraryStore::open(&library, &state).expect("migrates");
    let hits = store
        .search(
            "renewal",
            &SearchFilter {
                template: Some("person".into()),
                since: Some("2026-10-01".into()),
                ..SearchFilter::default()
            },
        )
        .expect("search");
    let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    assert_eq!(ids, ["dom-person-jane"]);
    let version: u32 = rusqlite::Connection::open(state.join("index.sqlite"))
        .expect("reopen")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 2);
}

#[test]
fn a_version_two_index_with_null_template_and_date_backfills_on_open() {
    // A Library already migrated to schema 2 by a build that did not
    // backfill: columns exist, rows are NULL, user_version is already 2.
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "knowledge/domain/dom-person-jane.md",
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\nupdated_at: 2026-10-20T10:00:00Z\n---\nRenewal owner.\n",
    );
    std::fs::create_dir_all(&state).expect("state");
    {
        let conn = rusqlite::Connection::open(state.join("index.sqlite")).expect("v2 index");
        conn.execute_batch(
            "CREATE TABLE documents (id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
                 summary TEXT NOT NULL, tags TEXT NOT NULL, series TEXT, sensitivity TEXT NOT NULL,
                 path TEXT NOT NULL UNIQUE, body TEXT NOT NULL, template TEXT, date TEXT);
             CREATE INDEX documents_kind ON documents(kind);
             CREATE INDEX documents_series ON documents(series);
             CREATE INDEX documents_template ON documents(template);
             CREATE INDEX documents_date ON documents(date);
             CREATE TABLE cites (doc_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 target TEXT NOT NULL, PRIMARY KEY (doc_id, target));
             CREATE TABLE links (from_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 to_id TEXT NOT NULL, PRIMARY KEY (from_id, to_id));
             CREATE VIRTUAL TABLE documents_fts USING fts5(id UNINDEXED, title, summary, body, tags);
             INSERT INTO documents (id, kind, title, summary, tags, series, sensitivity, path, body, template, date)
                 VALUES ('dom-person-jane', 'domain', 'Jane', '', '[]', NULL, 'normal',
                         'knowledge/domain/dom-person-jane.md', 'Renewal owner.', NULL, NULL);
             INSERT INTO documents_fts (id, title, summary, body, tags)
                 VALUES ('dom-person-jane', 'Jane', '', 'Renewal owner.', '');
             PRAGMA user_version = 2;",
        )
        .expect("v2 schema with stale row");
    }
    let store = LibraryStore::open(&library, &state).expect("open backfills");
    let hits = store
        .search(
            "renewal",
            &SearchFilter {
                template: Some("person".into()),
                since: Some("2026-10-01".into()),
                until: Some("2026-10-31".into()),
                ..SearchFilter::default()
            },
        )
        .expect("search");
    let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    assert_eq!(ids, ["dom-person-jane"]);
    // A second open must not loop: the backfill marker is cleared.
    drop(store);
    let store = LibraryStore::open(&library, &state).expect("second open");
    assert_eq!(store.document_count().expect("count"), 1);
}

#[test]
fn a_failed_backfill_on_open_is_retried_by_the_next_open() {
    // Same stale schema-2 index as above, plus a document the scan cannot
    // read: the first open must fail its backfill, and the marker must
    // survive so the next open retries instead of never healing.
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "knowledge/domain/dom-person-jane.md",
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\nupdated_at: 2026-10-20T10:00:00Z\n---\nRenewal owner.\n",
    );
    std::fs::create_dir_all(&state).expect("state");
    {
        let conn = rusqlite::Connection::open(state.join("index.sqlite")).expect("v2 index");
        conn.execute_batch(
            "CREATE TABLE documents (id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
                 summary TEXT NOT NULL, tags TEXT NOT NULL, series TEXT, sensitivity TEXT NOT NULL,
                 path TEXT NOT NULL UNIQUE, body TEXT NOT NULL, template TEXT, date TEXT);
             CREATE TABLE cites (doc_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 target TEXT NOT NULL, PRIMARY KEY (doc_id, target));
             CREATE TABLE links (from_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 to_id TEXT NOT NULL, PRIMARY KEY (from_id, to_id));
             CREATE VIRTUAL TABLE documents_fts USING fts5(id UNINDEXED, title, summary, body, tags);
             INSERT INTO documents (id, kind, title, summary, tags, series, sensitivity, path, body, template, date)
                 VALUES ('dom-person-jane', 'domain', 'Jane', '', '[]', NULL, 'normal',
                         'knowledge/domain/dom-person-jane.md', 'Renewal owner.', NULL, NULL);
             INSERT INTO documents_fts (id, title, summary, body, tags)
                 VALUES ('dom-person-jane', 'Jane', '', 'Renewal owner.', '');
             PRAGMA user_version = 2;",
        )
        .expect("v2 schema with stale row");
    }
    let broken = library.join("knowledge/domain/broken.md");
    std::fs::write(&broken, [0xff, 0xfe, b'\n']).expect("unreadable document");
    assert!(LibraryStore::open(&library, &state).is_err());
    std::fs::remove_file(&broken).expect("remove broken document");
    let store = LibraryStore::open(&library, &state).expect("retry backfills");
    let hits = store
        .search(
            "renewal",
            &SearchFilter {
                template: Some("person".into()),
                since: Some("2026-10-01".into()),
                ..SearchFilter::default()
            },
        )
        .expect("search");
    let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    assert_eq!(ids, ["dom-person-jane"]);
}

fn artifact_candidate() -> office_core::domain::artifact::Artifact {
    use office_core::domain::artifact::{Approval, Approver, Artifact, ArtifactManifest, Producer};
    Artifact::approved(
        serde_json::from_value::<ArtifactManifest>(serde_json::json!({
            "kind":"report","title":"Promoted output","files":["report.md","data/results.bin"],
            "series":"fixture-series","summary":"Approved fixture", "cites":[]
        }))
        .unwrap(),
        "<id>".into(),
        Producer {
            crew: Some("fixture-crew".into()),
            runbook: Some("rb-research".into()),
            at: Some("2026-10-27T10:00:00Z".into()),
        },
        Approval {
            by: Approver::User,
            at: "2026-10-27T11:00:00Z".into(),
            edited: true,
        },
    )
    .unwrap()
}

fn artifact_files() -> Vec<office_core::domain::artifact::ArtifactFile> {
    use office_core::domain::artifact::ArtifactFile;
    vec![
        ArtifactFile {
            path: "report.md".into(),
            bytes: b"Approved report\n".to_vec(),
        },
        ArtifactFile {
            path: "data/results.bin".into(),
            bytes: vec![0, 255, 128, 3],
        },
    ]
}

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
