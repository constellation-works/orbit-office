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
fn status_reports_an_empty_secret_as_unset() {
    let fx = Fixture::new();
    let request = serde_json::json!({"schema_version": 1, "tool": "office.status", "input": {}, "context": {"secrets": {"meta_api_key": ""}}})
        .to_string();
    let out = fx.ok(&["orbit-tool"], &request);
    let response = json(&out.stdout);
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
fn qa_exec_fixture_runs_init_in_its_temp_root() {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    let control = tempfile::tempdir().expect("control root");
    let input = control.path().join("input.json");
    std::fs::write(&input, "{}").expect("write input");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let run_fixture = |extra: &[String]| {
        Command::new("make")
            .args(["--no-print-directory", "-C"])
            .arg(&repo)
            .arg("qa-exec")
            .args(extra)
            .env("TMPDIR", control.path())
            .env_remove("ORBIT_OPERATOR")
            .output()
            .expect("run make qa-exec")
    };
    let base_args = vec![
        "TOOL=office.init".to_string(),
        format!("INPUT={}", input.display()),
        format!("QA_EXEC_OFFICE_BIN={}", support::BIN),
    ];
    let out = run_fixture(&base_args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "qa-exec failed: stdout={} stderr={}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let stdout = text(&out.stdout);
    let audit_line = stdout.lines().next().expect("audit line on stdout");
    assert!(
        audit_line.starts_with("qa-exec audit tool=office.init root="),
        "audit line: {audit_line}"
    );
    let root = PathBuf::from(
        audit_line
            .split(" root=")
            .nth(1)
            .and_then(|rest| rest.split(" time=").next())
            .expect("audit line names the temp root"),
    );
    let response_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .expect("response");
    let response = json(response_line.as_bytes());
    assert_eq!(
        response["ok"],
        serde_json::json!(true),
        "response: {response}"
    );
    assert_eq!(
        response["output"]["already_initialized"],
        serde_json::json!(false)
    );
    let audit_file = std::fs::read_to_string(root.join("qa-exec-audit.log")).expect("audit file");
    assert!(
        audit_file.contains("tool=office.init"),
        "audit file: {audit_file}"
    );
    assert!(
        audit_file.contains(&format!("root={}", root.display())),
        "audit file names the root: {audit_file}"
    );
    assert!(root.join("workspace/library/.office/config.yaml").is_file());
    assert!(root.join("workspace/library/.git").exists());
    let request = json(&std::fs::read(root.join("request.json")).expect("fixture request"));
    let context = request["context"].as_object().expect("context object");
    let mut keys: Vec<&str> = context.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["library_dir", "plugin_state", "workspace_root"]);
    for value in context.values() {
        let value = value.as_str().expect("root context value");
        assert!(
            Path::new(value).starts_with(&root),
            "context root {value} stays under {}",
            root.display()
        );
    }
    // Every file the run wrote is either the caller's input file or
    // under the fixture root.
    let mut stack = vec![control.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read control dir") {
            let path = entry.expect("control entry").path();
            if path.is_dir() && path != root {
                stack.push(path.clone());
            }
            if path.is_file() {
                assert!(
                    path == input || path.starts_with(&root),
                    "fixture wrote outside its root: {}",
                    path.display()
                );
            }
        }
    }
    // A root the fixture did not create is refused and untouched.
    let outside = control.path().join("outside-root");
    std::fs::create_dir(&outside).expect("outside root");
    let mut refused_args = base_args.clone();
    refused_args.push(format!("QA_EXEC_ROOT={}", outside.display()));
    let refused = run_fixture(&refused_args);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "outside root must be refused"
    );
    assert!(
        text(&refused.stderr).contains("refusing"),
        "refusal: {}",
        text(&refused.stderr)
    );
    assert!(!outside.join("workspace").exists());
    // An absolute input path outside the fixture root is refused too.
    let escape_input = control.path().join("escape.json");
    let escape_target = control.path().join("outside-library");
    std::fs::write(
        &escape_input,
        serde_json::json!({"library_dir": escape_target}).to_string(),
    )
    .expect("write escape input");
    let escape = run_fixture(&[
        "TOOL=office.init".to_string(),
        format!("INPUT={}", escape_input.display()),
        format!("QA_EXEC_OFFICE_BIN={}", support::BIN),
    ]);
    assert_ne!(
        escape.status.code(),
        Some(0),
        "outside input path must be refused"
    );
    assert!(
        text(&escape.stderr).contains("outside the fixture root"),
        "refusal: {} {}",
        text(&escape.stderr),
        text(&escape.stdout)
    );
    assert!(!escape_target.exists());
}

#[test]
fn status_does_not_open_library() {
    let workspace = tempfile::tempdir().expect("workspace");
    let request = serde_json::json!({"schema_version": 1, "tool": "office.status", "input": {}, "context": {"workspace_root": workspace.path()}}).to_string();
    let output = Fixture::new().ok(&["orbit-tool"], &request);
    assert_eq!(json(&output.stdout)["ok"], true);
    assert!(!workspace.path().join("library").exists());
}
