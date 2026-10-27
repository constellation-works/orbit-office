//! End-to-end: a Library on disk, opened through the facade, reindexed
//! and searched — the flow `office.reindex` and `office.search` will use.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_store::{DocumentKind, LibraryStore, SearchFilter};
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
    assert_eq!(
        store.read_document(Path::new(path)).unwrap_err().code(),
        "library_write_pending"
    );
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
