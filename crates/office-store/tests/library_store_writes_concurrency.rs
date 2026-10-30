//! Journal, writer-lock and concurrent-write scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::{LibraryStore, SearchFilter};
use std::path::Path;

#[test]
fn source_processing_journal_recovers_and_is_removed() {
    use office_core::domain::endpoint::ModelRoute;
    use office_core::domain::source::sha256_hex;
    use office_core::ports::{CaptureSourceRequest, DerivedBy, Library};

    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let captured = store
        .capture_source(&CaptureSourceRequest {
            captured_at: "2026-10-27T10:00:00+00:00".into(),
            kind: Some("meeting".into()),
            title: Some("Meeting".into()),
            text: Some("Original transcript".into()),
            ..Default::default()
        })
        .unwrap();
    let markdown = store.read_text(Path::new(&captured.path)).unwrap();
    let index_lock = library.join(".git/index.lock");
    std::fs::write(&index_lock, "fixture lock").unwrap();
    assert_eq!(
        Library::write_source_processing(
            &store,
            &captured.id,
            &sha256_hex(markdown.as_bytes()),
            Some("Recovered transcript"),
            &DerivedBy {
                route: ModelRoute::Hosted,
                model: "fixture".into(),
            },
            &[("chapters.md".into(), b"## Chapter 1\n".to_vec())],
        )
        .unwrap_err()
        .code(),
        "library_git_error"
    );

    let journal_path = state.join("library.pending-write.json");
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    assert_eq!(journal["version"], 7);
    assert_eq!(journal["kind"], "source_processing");
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");

    std::fs::remove_file(index_lock).unwrap();
    drop(store);
    let recovered = LibraryStore::open(&library, &state).expect("recover source processing");
    assert!(
        recovered
            .read_text(Path::new(&captured.path))
            .unwrap()
            .contains("Recovered transcript")
    );
    assert_eq!(
        Library::source_sidecar(&recovered, &captured.id, "chapters.md").unwrap(),
        Some("## Chapter 1\n".into())
    );
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert!(!journal_path.exists());
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
