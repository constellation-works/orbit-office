//! `office.search` through dispatch against the fake Library (STD-04):
//! each filter narrows the hits, and malformed input is refused.

use std::path::Path;

use office_core::ports::{Library, Ports};
use office_testkit::fakes::{FakeClock, FakeLibrary, FakeModels, FakeOrbit};
use serde_json::{Value, json};

fn ports() -> Ports<'static> {
    let library = FakeLibrary::initialized();
    for (path, text) in [
        (
            "sources/2026/10/src-20261028-acme-qbr/source.md",
            "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\noccurred_at: 2026-10-28T14:00:00-04:00\ntags: [client:acme]\n---\n[00:00:01] Jane: Renewal pricing is firm.\n",
        ),
        (
            "sources/2026/09/src-20260915-beta-call/source.md",
            "---\nid: src-20260915-beta-call\nkind: meeting\ntitle: Beta call\ncaptured_at: 2026-09-15T09:00:00Z\ntags: [client:beta]\n---\n[00:00:01] Bo: Renewal pricing is open.\n",
        ),
        (
            "knowledge/domain/dom-person-jane-park.md",
            "---\nid: dom-person-jane-park\ntemplate: person\ntitle: Jane Park\nupdated_at: 2026-10-20T10:00:00Z\n---\n## Role\nDecides renewal pricing.\n",
        ),
        (
            "knowledge/domain/dom-client-acme.md",
            "---\nid: dom-client-acme\ntemplate: client\ntitle: Acme\ntags: [client:acme]\n---\n## Overview\nRenewal pricing account.\n",
        ),
        (
            "knowledge/runbooks/rb-renewal-pricing.md",
            "---\nid: rb-renewal-pricing\ntemplate: runbook\ntitle: Renewal pricing\n---\n## Inputs\nRenewal pricing inputs.\n",
        ),
        (
            "artifacts/2026/10/art-20261021-renewal-pricing/artifact.md",
            "---\nid: art-20261021-renewal-pricing\nkind: report\ntitle: Renewal pricing report\nseries: renewal-pricing\n---\nRenewal pricing summary.\n",
        ),
        (
            "artifacts/2026/07/art-20260721-renewal-pricing/artifact.md",
            "---\nid: art-20260721-renewal-pricing\nkind: report\ntitle: Renewal pricing report\nseries: other-series\n---\nRenewal pricing summary.\n",
        ),
    ] {
        library
            .write_document(Path::new(path), text)
            .expect("seed document");
    }
    Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(FakeModels::new()),
        orbit: Box::new(FakeOrbit::default()),
        clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

fn search(ports: &Ports<'_>, input: Value) -> Value {
    let raw = office_testkit::envelope::request("office.search", input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn ids(response: &Value) -> Vec<String> {
    assert_eq!(response["ok"], json!(true), "{response}");
    let mut ids: Vec<String> = response["output"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .filter_map(|hit| hit["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    assert_eq!(response["output"]["count"], json!(ids.len()));
    ids
}

#[test]
fn an_unfiltered_query_finds_every_layer() {
    let ports = ports();
    let all = ids(&search(&ports, json!({ "query": "renewal pricing" })));
    assert_eq!(all.len(), 7, "{all:?}");
}

#[test]
fn each_filter_narrows_the_hits() {
    let ports = ports();
    let narrowed = |filter: Value| {
        let mut input = json!({ "query": "renewal pricing" });
        input
            .as_object_mut()
            .expect("object")
            .extend(filter.as_object().expect("filter").clone());
        ids(&search(&ports, input))
    };
    assert_eq!(
        narrowed(json!({ "kind": "source" })),
        ["src-20260915-beta-call", "src-20261028-acme-qbr"]
    );
    assert_eq!(
        narrowed(json!({ "kind": "runbook" })),
        ["rb-renewal-pricing"]
    );
    assert_eq!(
        narrowed(json!({ "template": "person" })),
        ["dom-person-jane-park"]
    );
    assert_eq!(
        narrowed(json!({ "tag": "client:acme" })),
        ["dom-client-acme", "src-20261028-acme-qbr"]
    );
    assert_eq!(
        narrowed(json!({ "series": "renewal-pricing" })),
        ["art-20261021-renewal-pricing"]
    );
    // occurred_at, captured_at, updated_at and the dated id each date a hit;
    // the undated client page and runbook drop out of any date filter.
    assert_eq!(
        narrowed(json!({ "since": "2026-10-01" })),
        [
            "art-20261021-renewal-pricing",
            "dom-person-jane-park",
            "src-20261028-acme-qbr"
        ]
    );
    assert_eq!(
        narrowed(json!({ "until": "2026-09-30" })),
        ["art-20260721-renewal-pricing", "src-20260915-beta-call"]
    );
    assert_eq!(
        narrowed(json!({ "since": "2026-10-21", "until": "2026-10-21" })),
        ["art-20261021-renewal-pricing"]
    );
    assert_eq!(narrowed(json!({ "limit": 2 })).len(), 2);
    assert!(narrowed(json!({ "template": "glossary" })).is_empty());
}

#[test]
fn an_empty_or_blank_query_is_refused() {
    let ports = ports();
    for input in [
        json!({}),
        json!({ "query": "" }),
        json!({ "query": "   " }),
        json!({ "query": 3 }),
    ] {
        let response = search(&ports, input.clone());
        assert_eq!(
            response["error"]["code"],
            json!("invalid_argument"),
            "{input}"
        );
    }
}

#[test]
fn malformed_filters_and_unknown_keys_are_refused() {
    let ports = ports();
    for input in [
        json!({ "query": "renewal", "kind": "page" }),
        json!({ "query": "renewal", "since": "2026-13-01" }),
        json!({ "query": "renewal", "until": "yesterday" }),
        json!({ "query": "renewal", "since": "2026-10-02", "until": "2026-10-01" }),
        json!({ "query": "renewal", "limit": 0 }),
        json!({ "query": "renewal", "limit": 101 }),
        json!({ "query": "renewal", "template": "" }),
        json!({ "query": "renewal", "date": "2026-10-01" }),
    ] {
        let response = search(&ports, input.clone());
        assert_eq!(
            response["error"]["code"],
            json!("invalid_argument"),
            "{input}"
        );
    }
}
