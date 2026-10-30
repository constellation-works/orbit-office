//! Reindex and search scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::{DocumentKind, Error, LibraryStore, SearchFilter};
use std::path::Path;

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
