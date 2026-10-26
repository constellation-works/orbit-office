//! Protocol dispatch: envelopes, unknown tools and unknown input keys.

use crate::serve;
use serde_json::{Value, json};

fn call(tool: &str, input: Value) -> Value {
    let raw = json!({
        "schema_version": 1,
        "tool": tool,
        "input": input,
        "context": {},
    })
    .to_string();
    serde_json::from_str(&serve(&raw)).expect("response is JSON")
}

#[test]
fn status_ok() {
    let response = call("office.status", json!({}));
    assert_eq!(response["ok"], json!(true));
    assert_eq!(response["output"]["version"], json!(crate::VERSION));
    assert_eq!(
        response["output"]["secrets"]["meta_api_key"],
        json!("unset")
    );
}

#[test]
fn unknown_tool_is_refused() {
    let response = call("office.nope", json!({}));
    assert_eq!(response["ok"], json!(false));
    assert_eq!(response["error"]["code"], json!("unknown_tool"));
}

#[test]
fn wrong_namespace_is_refused() {
    let response = call("other.status", json!({}));
    assert_eq!(response["ok"], json!(false));
    assert_eq!(response["error"]["code"], json!("unknown_tool"));
}

#[test]
fn unknown_input_key_is_refused() {
    let response = call("office.status", json!({ "verbose": true }));
    assert_eq!(response["ok"], json!(false));
    assert_eq!(response["error"]["code"], json!("invalid_argument"));
}

#[test]
fn bad_json_is_refused() {
    let response: Value = serde_json::from_str(&serve("not json")).expect("response is JSON");
    assert_eq!(response["ok"], json!(false));
    assert_eq!(response["error"]["code"], json!("invalid_request"));
}

#[test]
fn wrong_schema_version_is_refused() {
    let raw = json!({ "schema_version": 2, "tool": "office.status", "input": {}, "context": {} })
        .to_string();
    let response: Value = serde_json::from_str(&serve(&raw)).expect("response is JSON");
    assert_eq!(response["ok"], json!(false));
    assert_eq!(
        response["error"]["code"],
        json!("unsupported_schema_version")
    );
}
