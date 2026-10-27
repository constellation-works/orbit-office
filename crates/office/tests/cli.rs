//! End-to-end tests of the built binary: the exec protocol over stdio,
//! streams and exit codes (STD-01 R12, R13, R20).
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use support::{Fixture, text};

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("one JSON document")
}

const STATUS_REQUEST: &str =
    r#"{"schema_version":1,"tool":"office.status","input":{},"context":{}}"#;

#[test]
fn status_round_trip() {
    let fx = Fixture::new();
    let out = fx.ok(&["orbit-tool"], STATUS_REQUEST);
    let response = json(&out.stdout);
    assert_eq!(response["ok"], serde_json::json!(true));
    assert_eq!(
        response["output"]["secrets"]["meta_api_key"],
        serde_json::json!("unset")
    );
    assert!(out.stderr.is_empty(), "stderr: {}", text(&out.stderr));
}

#[test]
fn an_unknown_tool_is_an_error_envelope_with_exit_zero() {
    // The protocol reports tool failures in the envelope; the process
    // itself served the call, so it exits 0.
    let fx = Fixture::new();
    let out = fx.ok(
        &["orbit-tool"],
        r#"{"schema_version":1,"tool":"office.nope","input":{},"context":{}}"#,
    );
    let response = json(&out.stdout);
    assert_eq!(response["ok"], serde_json::json!(false));
    assert_eq!(response["error"]["code"], serde_json::json!("unknown_tool"));
}

#[test]
fn a_usage_error_exits_two_on_stderr() {
    let fx = Fixture::new();
    let out = fx.run(&["note", "list"], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn status_does_not_open_library() {
    let workspace = tempfile::tempdir().expect("workspace");
    let request = serde_json::json!({"schema_version": 1, "tool": "office.status", "input": {}, "context": {"workspace_root": workspace.path()}}).to_string();
    let output = Fixture::new().ok(&["orbit-tool"], &request);
    assert_eq!(json(&output.stdout)["ok"], true);
    assert!(!workspace.path().join("library").exists());
}
