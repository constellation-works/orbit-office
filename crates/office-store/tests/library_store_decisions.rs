//! Upload, capture and settings decision scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::LibraryStore;
use std::path::Path;

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
