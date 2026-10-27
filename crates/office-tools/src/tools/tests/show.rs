//! `office.source-show` through dispatch against the fake Library (STD-04).

use serde_json::{Value, json};

fn call(tool: &str, input: Value, ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

#[test]
fn source_show_returns_line_anchors_and_refuses_bad_anchors() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({"kind": "chat", "title": "Thread", "text": "one\ntwo\nthree\nfour"}),
        &ports,
    );
    let id = captured["output"]["id"].as_str().expect("id").to_string();
    let span = call(
        "office.source-show",
        json!({"id": id, "anchor": "#L2-3"}),
        &ports,
    );
    assert_eq!(span["output"]["body"], json!("two\nthree"), "{span}");
    let malformed = call(
        "office.source-show",
        json!({"id": id, "anchor": "banana"}),
        &ports,
    );
    assert_eq!(malformed["error"]["code"], json!("anchor_invalid"));
    let out_of_range = call(
        "office.source-show",
        json!({"id": id, "anchor": "#L99"}),
        &ports,
    );
    assert_eq!(out_of_range["error"]["code"], json!("anchor_out_of_range"));
    assert_ne!(
        malformed["error"]["code"], out_of_range["error"]["code"],
        "malformed and out-of-range anchors have distinct codes"
    );
    let missing = call(
        "office.source-show",
        json!({"id": "src-00000000-nope"}),
        &ports,
    );
    assert_eq!(missing["error"]["code"], json!("source_not_found"));
}

#[test]
fn source_show_resolves_interior_time_anchor_to_turn_in_progress() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({
            "kind": "meeting",
            "title": "Meeting",
            "text": "[00:10:00] Jane: one\n\n[00:12:31] Dee: two"
        }),
        &ports,
    );
    let id = captured["output"]["id"].as_str().expect("id").to_string();
    let span = call(
        "office.source-show",
        json!({"id": id, "anchor": "#t=00:11:00"}),
        &ports,
    );
    assert_eq!(span["ok"], json!(true), "{span}");
    assert_eq!(
        span["output"]["body"],
        json!("[00:10:00] Jane: one"),
        "{span}"
    );
}
