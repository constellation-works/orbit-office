//! The SQLite driver: open/migrate, rebuild, upsert, search, get.

use crate::contracts::{DocumentKind, DocumentRecord, LinkKind, SearchFilter};
use crate::driver::sqlite::connection::IndexStore;

fn document(id: &str, title: &str, body: &str) -> DocumentRecord {
    DocumentRecord {
        shareable: false,
        id: id.to_string(),
        kind: DocumentKind::Domain,
        title: title.to_string(),
        summary: String::new(),
        tags: vec!["client:acme".to_string()],
        series: None,
        sensitivity: "normal".to_string(),
        path: format!("knowledge/domain/{id}.md"),
        body: body.to_string(),
        cites: vec!["src-20261028-acme-qbr#t=00:12:31".to_string()],
        related: vec!["rb-client-qbr-prep".to_string()],
        template: None,
        date: None,
    }
}

#[test]
fn rebuild_get_and_search_round_trip() {
    let index = IndexStore::open_in_memory().expect("open");
    index
        .rebuild(&[
            document("dom-acme", "Acme", "Renewal pricing stays firm this year."),
            document("dom-beta", "Beta", "Beta onboarding notes."),
        ])
        .expect("rebuild");
    assert_eq!(index.count().expect("count"), 2);

    let got = index.get("dom-acme").expect("get").expect("present");
    assert_eq!(got.title, "Acme");
    assert_eq!(got.cites, vec!["src-20261028-acme-qbr#t=00:12:31"]);
    assert_eq!(got.related, vec!["rb-client-qbr-prep"]);

    let hits = index
        .search("renewal pricing", &SearchFilter::default())
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "dom-acme");

    let filtered = index
        .search(
            "notes",
            &SearchFilter {
                kind: Some(DocumentKind::Runbook),
                ..SearchFilter::default()
            },
        )
        .expect("filtered search");
    assert!(filtered.is_empty());
}

#[test]
fn upsert_replaces_without_duplicating() {
    let index = IndexStore::open_in_memory().expect("open");
    index
        .upsert(&document("dom-acme", "Acme", "first"))
        .expect("upsert");
    index
        .upsert(&document("dom-acme", "Acme renamed", "second"))
        .expect("replace");
    assert_eq!(index.count().expect("count"), 1);
    let hits = index
        .search("second", &SearchFilter::default())
        .expect("search");
    assert_eq!(hits.len(), 1);
}

#[test]
fn backlinks_come_from_cites_with_or_without_anchor_and_related() {
    let index = IndexStore::open_in_memory().expect("open");
    let mut page = document("dom-acme", "Acme", "Acme.");
    page.cites = vec![];
    page.related = vec![];
    let mut citing = document("dom-renewal", "Renewal", "Renewal.");
    citing.cites = vec!["dom-acme".into()];
    citing.related = vec!["dom-acme".into()];
    let mut source = document("src-20261028-acme-qbr", "QBR", "Meeting.");
    source.kind = DocumentKind::Source;
    source.path = "sources/2026/10/src-20261028-acme-qbr/source.md".into();
    source.cites = vec!["dom-acme#Overview".into()];
    source.related = vec![];
    let mut prefix_only = document("dom-other", "Other", "Other.");
    prefix_only.cites = vec!["dom-acme-two".into()];
    prefix_only.related = vec!["dom-acmex".into()];
    index
        .rebuild(&[page, citing, source, prefix_only])
        .expect("rebuild");

    let links = index.backlinks("dom-acme").expect("backlinks");
    let ids: Vec<&str> = links.iter().map(|link| link.id.as_str()).collect();
    assert_eq!(ids, ["dom-renewal", "src-20261028-acme-qbr"]);
    assert_eq!(links[0].via, [LinkKind::Cites, LinkKind::Related]);
    assert_eq!(links[1].via, [LinkKind::Cites]);
    assert_eq!(links[1].kind, DocumentKind::Source);
    assert!(index.backlinks("dom-absent").expect("none").is_empty());
}

#[test]
fn template_and_date_filters_narrow_before_the_limit() {
    let index = IndexStore::open_in_memory().expect("open");
    let mut person = document("dom-person-jane", "Jane", "Renewal owner.");
    person.template = Some("person".into());
    person.date = Some("2026-10-20".into());
    let mut client = document("dom-client-acme", "Acme", "Renewal account.");
    client.template = Some("client".into());
    let mut source = document("src-20260915-call", "Call", "Renewal call.");
    source.kind = DocumentKind::Source;
    source.path = "sources/2026/09/src-20260915-call/source.md".into();
    source.date = Some("2026-09-15".into());
    index.rebuild(&[person, client, source]).expect("rebuild");

    let ids = |filter: SearchFilter| {
        let mut ids: Vec<String> = index
            .search("renewal", &filter)
            .expect("search")
            .into_iter()
            .map(|hit| hit.id)
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(SearchFilter::default()).len(), 3);
    assert_eq!(
        ids(SearchFilter {
            template: Some("person".into()),
            ..SearchFilter::default()
        }),
        ["dom-person-jane"]
    );
    assert_eq!(
        ids(SearchFilter {
            since: Some("2026-10-01".into()),
            ..SearchFilter::default()
        }),
        ["dom-person-jane"]
    );
    assert_eq!(
        ids(SearchFilter {
            until: Some("2026-09-30".into()),
            ..SearchFilter::default()
        }),
        ["src-20260915-call"]
    );
    assert_eq!(
        ids(SearchFilter {
            template: Some("person".into()),
            limit: Some(1),
            ..SearchFilter::default()
        }),
        ["dom-person-jane"]
    );
    let got = index.get("dom-person-jane").expect("get").expect("present");
    assert_eq!(got.template.as_deref(), Some("person"));
    assert_eq!(got.date.as_deref(), Some("2026-10-20"));
}

#[test]
fn an_empty_query_lists_the_filtered_documents_by_id() {
    let index = IndexStore::open_in_memory().expect("open");
    let mut older = document("art-20260721-deck", "Deck", "Older.");
    older.kind = DocumentKind::Artifact;
    older.path = "artifacts/2026/07/art-20260721-deck/artifact.md".into();
    older.series = Some("deck".into());
    let mut newer = older.clone();
    newer.id = "art-20261021-deck".into();
    newer.path = "artifacts/2026/10/art-20261021-deck/artifact.md".into();
    let mut other = older.clone();
    other.id = "art-20261022-memo".into();
    other.path = "artifacts/2026/10/art-20261022-memo/artifact.md".into();
    other.series = Some("memo".into());
    index
        .rebuild(&[document("dom-acme", "Acme", "Acme."), newer, older, other])
        .expect("rebuild");
    let ids: Vec<String> = index
        .search(
            " ",
            &SearchFilter {
                kind: Some(DocumentKind::Artifact),
                series: Some("deck".into()),
                ..SearchFilter::default()
            },
        )
        .expect("list")
        .into_iter()
        .map(|hit| hit.id)
        .collect();
    assert_eq!(ids, ["art-20260721-deck", "art-20261021-deck"]);
}

fn bootstrap_store() -> (
    tempfile::TempDir,
    crate::LibraryStore,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let root = workspace.join("library");
    let state = dir.path().join("state");
    crate::LibraryStore::initialize_at(&root, &state, None, &workspace).unwrap();
    let store = crate::LibraryStore::open(&root, &state).unwrap();
    (dir, store, root, state)
}

#[test]
fn source_digest_is_indexed_under_source_id_on_processing_and_rebuild() {
    use office_core::domain::{endpoint::ModelRoute, source::sha256_hex};
    use office_core::ports::{DerivedBy, Library};
    let (_temp, store, root, _state) = bootstrap_store();
    let path = std::path::Path::new("sources/1970/01/src-doc/source.md");
    let text = "---\nid: src-doc\nkind: doc\ntitle: Design\nsensitivity: normal\n---\nOriginal evidence.\n";
    store.write_document(path, text).unwrap();
    let value = serde_json::json!({"summary":"Digest","kind_hint":"doc","as_of":null,
        "subjects":[{"name":"SidecarSubject","template":"topic","aliases":[]}],
        "claims":[],"procedures":[],"terms":[],"refs":[]});
    Library::write_source_processing(
        &store,
        "src-doc",
        &sha256_hex(text.as_bytes()),
        None,
        &DerivedBy {
            route: ModelRoute::Hosted,
            model: "offline".into(),
        },
        &[("digest.json".into(), value.to_string().into_bytes())],
    )
    .unwrap();
    for rebuilt in [false, true] {
        if rebuilt {
            store.reindex().unwrap();
        }
        let hits = store
            .search("SidecarSubject", &SearchFilter::default())
            .unwrap();
        assert_eq!(hits.len(), 1, "rebuilt={rebuilt}");
        assert_eq!(hits[0].id, "src-doc");
        assert_eq!(hits[0].path, path.to_string_lossy());
        assert!(
            !std::fs::read_to_string(root.join(path))
                .unwrap()
                .contains("SidecarSubject")
        );
    }
}

#[test]
fn bootstrap_public_port_persists_private_plan_confirms_once_and_rejects_stale_snapshots() {
    use office_core::{
        domain::bootstrap::{self, Mutation},
        ports::Library,
    };
    use serde_json::json;
    let (_dir, store, root, state) = bootstrap_store();
    store.set_setting("open_weights",json!({"endpoint":"https://private.example/v1","model":"fixture-private","kind":"company","label":"Fixture server"})).unwrap();
    store
        .record_route_check(
            "https://private.example/v1",
            "fixture-private",
            &[office_core::domain::endpoint::Capability::Text],
            "1970-01-01T00:00:00Z",
        )
        .unwrap();
    for id in ["one", "two"] {
        store.write_document(std::path::Path::new(&format!("sources/1970/01/{id}/source.md")),&format!("---\nid: {id}\nkind: doc\ntags: [bootstrap:docs]\nsensitivity: normal\n---\n# Ingest\nContent.\n")).unwrap();
    }
    let snapshot = store.bootstrap_snapshot().unwrap();
    let settings = store.settings().unwrap();
    let revision = snapshot.revision(&settings).unwrap();
    let plan = bootstrap::plan(&snapshot, &settings, "docs", true, vec![]).unwrap();
    let preview = Mutation::Plan {
        plan: Box::new(plan.clone()),
        confirm: None,
        at: "1970-01-01T00:00:00Z".into(),
    };
    let saved = store.mutate_batch(&revision, preview.clone()).unwrap();
    assert_eq!(saved.plan, plan);
    assert_eq!(
        store.mutate_batch(&revision, preview).unwrap_err().code(),
        "plan_changed"
    );
    for id in ["one", "two"] {
        assert_eq!(store.get(id).unwrap().unwrap().sensitivity, "private");
    }
    drop(store);
    let store = crate::LibraryStore::open(&root, &state).unwrap();
    let snapshot = store.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.batches["docs"], saved);
    let confirm = Mutation::Plan {
        plan: Box::new(plan.clone()),
        confirm: Some(plan.digest),
        at: "1970-01-01T00:00:00Z".into(),
    };
    let confirmed = store
        .mutate_batch(&snapshot.revision(&settings).unwrap(), confirm.clone())
        .unwrap();
    let head = store.head_commit().unwrap();
    let snapshot = store.bootstrap_snapshot().unwrap();
    assert_eq!(
        store
            .mutate_batch(&snapshot.revision(&settings).unwrap(), confirm)
            .unwrap(),
        confirmed
    );
    assert_eq!(store.head_commit().unwrap(), head);
    store.reindex().unwrap();
    assert_eq!(store.get("one").unwrap().unwrap().sensitivity, "private");
}

#[test]
fn bootstrap_snapshot_reads_complete_digest_corpus_and_refuses_invalid_sidecars() {
    use office_core::ports::Library;
    let (_dir, store, root, _state) = bootstrap_store();
    let directory = root.join("sources/1970/01/one");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("source.md"),
        "---\nid: one\nkind: doc\ntags: [bootstrap:docs]\n---\nMaterial.\n",
    )
    .unwrap();
    let value = serde_json::json!({"summary":"Digest","subjects":[],"claims":[],"procedures":[],"terms":[],"refs":[]});
    std::fs::write(directory.join("digest.json"), value.to_string()).unwrap();
    let before = store.head_commit().unwrap();
    assert_eq!(
        store.bootstrap_snapshot().unwrap().sources[0].digest,
        Some(value)
    );
    assert_eq!(store.head_commit().unwrap(), before);
    assert!(!root.join(".office/bootstrap.json").exists());
    std::fs::write(directory.join("digest.json"), "{bad").unwrap();
    assert_eq!(
        store.bootstrap_snapshot().unwrap_err().code(),
        "invalid_bootstrap_data"
    );
    std::fs::write(directory.join("digest.json"), [0xff]).unwrap();
    assert_eq!(store.bootstrap_snapshot().unwrap_err().code(), "io_error");
}

#[cfg(unix)]
#[test]
fn bootstrap_snapshot_refuses_digest_and_ledger_symlinks() {
    use office_core::ports::Library;
    use std::os::unix::fs::symlink;
    let (dir, store, root, _state) = bootstrap_store();
    let directory = root.join("sources/one");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("source.md"),
        "---\nid: one\nkind: doc\n---\nMaterial.\n",
    )
    .unwrap();
    let outside = dir.path().join("outside.json");
    std::fs::write(&outside, "{}").unwrap();
    symlink(&outside, directory.join("digest.json")).unwrap();
    assert_eq!(
        store.bootstrap_snapshot().unwrap_err().code(),
        "unsafe_path"
    );
    // Rename the fixture link out of the source tree; don't follow or remove it.
    std::fs::rename(
        directory.join("digest.json"),
        dir.path().join("digest-link"),
    )
    .unwrap();
    symlink(&outside, root.join(".office/bootstrap.json")).unwrap();
    assert_eq!(
        store.bootstrap_snapshot().unwrap_err().code(),
        "unsafe_path"
    );
}

#[test]
fn bootstrap_journal_recovery_finishes_privacy_and_ledger_together() {
    use office_core::{
        domain::bootstrap::{self, Mutation},
        ports::Library,
    };
    use serde_json::json;
    let (_dir, store, root, state) = bootstrap_store();
    let relative = "sources/one/source.md";
    store
        .write_document(
            std::path::Path::new(relative),
            "---\nid: one\nkind: doc\ntags: [bootstrap:docs]\n---\n# Ingest\nContent.\n",
        )
        .unwrap();
    store.set_setting("open_weights",json!({"endpoint":"https://private.example/v1","model":"fixture-private","kind":"company","label":"Fixture server"})).unwrap();
    store
        .record_route_check(
            "https://private.example/v1",
            "fixture-private",
            &[office_core::domain::endpoint::Capability::Text],
            "1970-01-01T00:00:00Z",
        )
        .unwrap();
    let snapshot = store.bootstrap_snapshot().unwrap();
    let settings = store.settings().unwrap();
    let plan = bootstrap::plan(&snapshot, &settings, "docs", true, vec![]).unwrap();
    let write = bootstrap::apply(
        &snapshot,
        &settings,
        Mutation::Plan {
            plan: Box::new(plan),
            confirm: None,
            at: "1970-01-01T00:00:00Z".into(),
        },
    )
    .unwrap();
    let text = serde_json::to_string(&write.batches).unwrap();
    let private_source = write.sources[relative].clone();
    std::fs::write(state.join("library.pending-write.json"),json!({"version":6,"kind":"bootstrap","root":root,"relative":".office/bootstrap.json","text":text,"token":"123-456","bundle":[{"path":relative,"bytes":private_source.as_bytes()}]}).to_string()).unwrap();
    drop(store);
    let store = crate::LibraryStore::open(&root, &state).unwrap();
    assert_eq!(store.bootstrap_snapshot().unwrap().batches, write.batches);
    assert_eq!(store.get("one").unwrap().unwrap().sensitivity, "private");
    assert!(!state.join("library.pending-write.json").exists());
    let head = store.head_commit().unwrap();
    drop(store);
    let store = crate::LibraryStore::open(&root, &state).unwrap();
    assert_eq!(store.head_commit().unwrap(), head);
}
