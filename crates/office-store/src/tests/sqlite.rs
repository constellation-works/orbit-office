//! The SQLite driver: open/migrate, rebuild, upsert, search, get.

use crate::contracts::{DocumentKind, DocumentRecord, SearchFilter};
use crate::driver::sqlite::connection::IndexStore;

fn document(id: &str, title: &str, body: &str) -> DocumentRecord {
    DocumentRecord {
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
