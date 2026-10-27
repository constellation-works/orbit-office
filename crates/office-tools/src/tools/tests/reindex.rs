//! `office.reindex` through dispatch against the fake Library (STD-04):
//! the shared uninitialized refusal and a rebuild count.

use serde_json::{Value, json};

fn call(tool: &str, input: Value, ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn ports(library: office_testkit::fakes::FakeLibrary) -> office_core::ports::Ports<'static> {
    office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

#[test]
fn reindex_refuses_an_uninitialized_library_naming_init() {
    let response = call(
        "office.reindex",
        json!({}),
        &ports(office_testkit::fakes::FakeLibrary::default()),
    );
    assert_eq!(response["ok"], json!(false));
    assert_eq!(response["error"]["code"], json!("library_not_initialized"));
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("office.init")),
        "refusal names office.init: {response}"
    );
}

#[test]
fn reindex_reports_the_indexed_document_count() {
    let ports = ports(office_testkit::fakes::FakeLibrary::initialized());
    // Write through the port, as a built write tool would.
    ports
        .library
        .write_document(
            std::path::Path::new("knowledge/domain/dom-a.md"),
            "---\nid: dom-a\ntitle: A\n---\nsearchable alpha",
        )
        .expect("write");
    let response = call("office.reindex", json!({}), &ports);
    assert_eq!(response["ok"], json!(true));
    assert_eq!(response["output"]["indexed_documents"], json!(1));
    let hits = ports
        .library
        .search(
            "alpha",
            &office_core::domain::knowledge::SearchFilter::default(),
        )
        .expect("search after reindex");
    assert_eq!(hits.len(), 1);
}
