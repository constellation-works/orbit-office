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
