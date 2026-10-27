//! The SQLite driver: open/migrate, rebuild, upsert, search, get.

use crate::contracts::{DocumentKind, DocumentRecord, LinkKind, SearchFilter};
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
