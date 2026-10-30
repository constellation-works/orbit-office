//! Initialization and index-migration scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::{LibraryStore, SearchFilter};
use std::path::Path;

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

// The open lock descriptor is a handshake that the retry has passed its
// pre-lock work. The fixture then leaves the same partial layout an earlier
// failed initializer could have left while owning that lock.
#[cfg(target_os = "linux")]
#[test]
fn init_retry_does_not_claim_layout_created_by_the_previous_writer() {
    use std::fs::OpenOptions;
    use std::time::{Duration, Instant};

    for internal_state in [false, true] {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        let library = workspace.join("library");
        let state = if internal_state {
            library.join(".office/state")
        } else {
            dir.path().join("state")
        };
        // An obstructed git init leaves a real partial scaffold. Move it
        // aside until the retry is waiting, to control the interleaving.
        let partial = dir.path().join("partial");
        if !internal_state {
            std::fs::create_dir_all(&library).expect("library");
            std::fs::write(library.join(".git"), "obstruction").expect("obstruct git init");
            assert!(LibraryStore::initialize_at(&library, &state, None, &workspace).is_err());
            assert!(library.join("sources").is_dir());
            std::fs::remove_file(library.join(".git")).expect("remove fixture obstruction");
            std::fs::rename(&library, &partial).expect("stage partial scaffold");
        }
        std::fs::create_dir_all(&state).expect("state");
        let lock_path = state.join("library.writer.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .expect("lock file");
        lock.lock().expect("hold lock");
        std::thread::scope(|scope| {
            let retry =
                scope.spawn(|| LibraryStore::initialize_at(&library, &state, None, &workspace));
            let deadline = Instant::now() + Duration::from_secs(3);
            let observed = loop {
                let count = std::fs::read_dir("/proc/self/fd")
                    .expect("descriptors")
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        std::fs::read_link(entry.path()).is_ok_and(|target| target == lock_path)
                    })
                    .count();
                if count >= 2 {
                    break true;
                }
                if retry.is_finished() || Instant::now() >= deadline {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            if observed {
                if internal_state {
                    std::fs::create_dir_all(library.join("sources")).expect("partial sources");
                    std::fs::create_dir_all(library.join("knowledge/domain"))
                        .expect("partial knowledge");
                    std::fs::create_dir_all(library.join(".office")).expect("partial office");
                } else {
                    std::fs::rename(&partial, &library).expect("publish partial scaffold");
                }
            }
            drop(lock);
            let outcome = retry.join().expect("retry thread").expect("retry init");
            assert!(observed, "retry did not open the writer lock");
            for path in ["sources", "knowledge/domain", ".office"] {
                assert!(
                    !outcome.created.iter().any(|created| created == path),
                    "{path}: {:?}",
                    outcome.created
                );
            }
            if internal_state {
                assert!(
                    outcome
                        .created
                        .iter()
                        .any(|path| path == "knowledge/runbooks")
                );
                assert!(
                    outcome
                        .created
                        .iter()
                        .any(|path| path == "sources/.gitkeep")
                );
            } else {
                assert!(
                    !outcome
                        .created
                        .iter()
                        .any(|path| path == "knowledge/runbooks")
                );
                assert!(
                    !outcome
                        .created
                        .iter()
                        .any(|path| path == "sources/.gitkeep")
                );
            }
        });
    }
}

#[test]
fn init_reports_office_created_by_internal_state_bootstrap_only_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let library = workspace.join("library");
    let state = library.join(".office/state");
    let first =
        LibraryStore::initialize_at(&library, &state, None, &workspace).expect("first init");
    assert_eq!(
        first
            .created
            .iter()
            .filter(|path| *path == ".office")
            .count(),
        1
    );
    let second =
        LibraryStore::initialize_at(&library, &state, None, &workspace).expect("repeat init");
    assert!(second.already_initialized);
    assert!(second.created.is_empty());
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
    assert_eq!(version, 4);
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
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\nupdated_at: 2026-10-20T10:00:00Z\nshareable: true\n---\nRenewal owner.\n",
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
    assert!(
        store.get("dom-person-jane").unwrap().unwrap().shareable,
        "migration 3 backfills sharing from Markdown"
    );
    // A second open must not loop: the backfill marker is cleared.
    drop(store);
    let store = LibraryStore::open(&library, &state).expect("second open");
    assert_eq!(store.document_count().expect("count"), 1);
    assert!(store.get("dom-person-jane").unwrap().unwrap().shareable);
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
