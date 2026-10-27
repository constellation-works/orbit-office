//! `office.knowledge-show` through dispatch against the fake Library
//! (STD-04): the page's model view and the documents that link to it.

use std::path::Path;

use office_core::ports::{Library, Ports};
use office_testkit::fakes::FakeLibrary;
use serde_json::{Value, json};

fn ports() -> Ports<'static> {
    let library = FakeLibrary::initialized();
    for (path, text) in [
        (
            "knowledge/domain/dom-person-jane-park.md",
            "---\nid: dom-person-jane-park\ntemplate: person\ntitle: Jane Park\naliases: [Jane]\nrefs:\n  - { label: Work email, contact: jane@acme.example }\n  - { label: CRM, url: \"https://crm.example/jane\" }\n---\n## Role\nBuyer. Old note: jane@acme.example\n",
        ),
        (
            "sources/2026/10/src-20261028-acme-qbr/source.md",
            "---\nid: src-20261028-acme-qbr\ntitle: Acme QBR\nrelated: [dom-person-jane-park]\n---\n[00:00:01] Jane: Hello.\n",
        ),
        (
            "knowledge/domain/dom-client-acme.md",
            "---\nid: dom-client-acme\ntitle: Acme\ncites: [dom-person-jane-park]\n---\nAcme.\n",
        ),
        (
            "knowledge/domain/dom-unrelated.md",
            "---\nid: dom-unrelated\ntitle: Other\ncites: [dom-person-jane-parker]\n---\nOther.\n",
        ),
    ] {
        library
            .write_document(Path::new(path), text)
            .expect("seed document");
    }
    Ports {
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
    }
}

fn show(ports: &Ports<'_>, input: Value) -> Value {
    let raw = office_testkit::envelope::request("office.knowledge-show", input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

#[test]
fn show_returns_the_pages_and_sources_that_link_to_the_page() {
    let ports = ports();
    let response = show(&ports, json!({ "id": "dom-person-jane-park" }));
    assert_eq!(response["ok"], json!(true), "{response}");
    let ids: Vec<&str> = response["output"]["backlinks"]
        .as_array()
        .expect("backlinks")
        .iter()
        .filter_map(|link| link["id"].as_str())
        .collect();
    assert_eq!(ids, ["dom-client-acme", "src-20261028-acme-qbr"]);
    assert_eq!(
        response["output"]["backlinks"][1]["via"],
        json!(["related"])
    );
    assert_eq!(response["output"]["backlinks"][0]["via"], json!(["cites"]));
}

#[test]
fn show_withholds_contact_values_and_keeps_their_labels() {
    let ports = ports();
    let response = show(&ports, json!({ "id": "dom-person-jane-park" }));
    let page = &response["output"]["page"];
    assert_eq!(page["refs"][0]["label"], json!("Work email"));
    assert_eq!(page["refs"][0]["target"], json!("contact"));
    assert_eq!(page["refs"][0]["value"], Value::Null);
    assert_eq!(page["refs"][1]["value"], json!("https://crm.example/jane"));
    assert_eq!(page["aliases"], json!(["Jane"]));
    assert!(
        !response.to_string().contains("jane@acme.example"),
        "{response}"
    );
}

#[test]
fn show_refuses_an_unknown_page_and_a_non_page_id() {
    let ports = ports();
    let missing = show(&ports, json!({ "id": "dom-absent" }));
    assert_eq!(missing["error"]["code"], json!("page_not_found"));
    let source = show(&ports, json!({ "id": "src-20261028-acme-qbr" }));
    assert_eq!(source["error"]["code"], json!("invalid_argument"));
    let extra = show(&ports, json!({ "id": "dom-person-jane-park", "raw": true }));
    assert_eq!(extra["error"]["code"], json!("invalid_argument"));
}
