//! Fake Library operations through the same public port as real adapters.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::{
    domain::knowledge::{DocumentKind, SearchFilter},
    ports::Library,
};
use office_testkit::fakes::FakeLibrary;
use std::path::Path;

#[test]
fn fake_implements_read_write_filtered_search_history_settings_and_readiness() {
    let fake = FakeLibrary::initialized();
    let port: &dyn Library = &fake;
    let path = Path::new("knowledge/domain/example.md");
    assert!(port.is_initialized().expect("ready"));
    let document = port
        .write_document(
            path,
            "---\nid: dom-example\ntitle: Example\ntags: [client]\n---\nsearchable body",
        )
        .expect("write");
    assert_eq!(port.read_document(path).expect("read"), document);
    assert_eq!(port.history(path).expect("history").len(), 1);
    let filter = SearchFilter {
        kind: Some(DocumentKind::Domain),
        tag: Some("client".into()),
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(port.search("searchable", &filter).expect("search").len(), 1);
    assert!(port.search("absent", &filter).expect("no match").is_empty());
    port.settings().expect("valid settings");
    assert!(
        !FakeLibrary::default()
            .is_initialized()
            .expect("uninitialized")
    );
    assert!(FakeLibrary::default().read_document(path).is_err());
}
