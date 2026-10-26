//! Envelope parsing and rendering.

use crate::protocol::{parse, respond};
use crate::{Error, SCHEMA_VERSION};
use serde_json::{Value, json};

#[test]
fn a_full_envelope_parses() {
    let raw = json!({
        "schema_version": SCHEMA_VERSION,
        "tool": "office.status",
        "input": { "a": 1 },
        "context": { "secrets": {} },
    })
    .to_string();
    let request = parse(&raw).expect("parses");
    assert_eq!(request.tool, "office.status");
    assert_eq!(request.input["a"], json!(1));
    assert!(request.context.contains_key("secrets"));
}

#[test]
fn absent_input_and_context_are_empty_objects() {
    let raw = json!({ "schema_version": SCHEMA_VERSION, "tool": "office.status" }).to_string();
    let request = parse(&raw).expect("parses");
    assert!(request.input.is_empty());
    assert!(request.context.is_empty());
}

#[test]
fn bad_envelopes_are_refused_with_their_codes() {
    let cases = [
        ("not json".to_string(), "invalid_request"),
        (json!([1]).to_string(), "invalid_request"),
        (
            json!({ "schema_version": 2, "tool": "office.status" }).to_string(),
            "unsupported_schema_version",
        ),
        (
            json!({ "schema_version": 1, "tool": "office.status", "input": [] }).to_string(),
            "invalid_argument",
        ),
        (
            json!({ "schema_version": 1, "tool": "office.status", "context": 1 }).to_string(),
            "invalid_request",
        ),
    ];
    for (raw, code) in cases {
        let error = parse(&raw).expect_err("refused");
        assert_eq!(error.code(), code, "{raw}");
    }
}

#[test]
fn respond_renders_both_envelopes() {
    let ok: Value = serde_json::from_str(&respond(Ok(json!({ "x": 1 })))).expect("JSON");
    assert_eq!(ok, json!({ "ok": true, "output": { "x": 1 } }));
    let err: Value =
        serde_json::from_str(&respond(Err(Error::UnknownTool("office.nope".to_string()))))
            .expect("JSON");
    assert_eq!(err["ok"], json!(false));
    assert_eq!(err["error"]["code"], json!("unknown_tool"));
    assert_eq!(err["error"]["retryable"], json!(false));
}
