//! Dispatch: envelopes, unknown and planned tools, unknown input keys.

use crate::serve;
use serde_json::{Value, json};

fn call(tool: &str, input: Value) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&serve(
        &request,
        &office_testkit::fakes::PortsBuilder::default().build(),
    ))
    .expect("response is JSON")
}

#[test]
fn status_ok() {
    let response = call("office.status", json!({}));
    assert_eq!(response["ok"], json!(true));
    assert_eq!(response["output"]["version"], json!(office_core::VERSION));
}

#[test]
fn unknown_tool_is_refused() {
    let response = call("office.nope", json!({}));
    assert_eq!(response["error"]["code"], json!("unknown_tool"));
}

#[test]
fn wrong_namespace_is_refused() {
    let response = call("other.status", json!({}));
    assert_eq!(response["error"]["code"], json!("unknown_tool"));
}

#[test]
fn planned_tools_are_refused_as_unknown() {
    // A planned tool is never served (STD-01 R36), whatever its input.
    for tool in crate::tools().iter().filter(|tool| !tool.is_built()) {
        let name = format!("office.{}", tool.spec().name);
        let response = call(&name, json!({}));
        assert_eq!(response["ok"], json!(false), "{name}");
        assert_eq!(response["error"]["code"], json!("unknown_tool"), "{name}");
    }
}

#[test]
fn unknown_input_key_is_refused() {
    let response = call("office.status", json!({ "verbose": true }));
    assert_eq!(response["error"]["code"], json!("invalid_argument"));
}

#[test]
fn bad_envelope_is_refused() {
    let response: Value = serde_json::from_str(&office_core::protocol::respond(
        office_core::protocol::parse("not json").map(|_| json!({})),
    ))
    .expect("response is JSON");
    assert_eq!(response["error"]["code"], json!("invalid_request"));
}
