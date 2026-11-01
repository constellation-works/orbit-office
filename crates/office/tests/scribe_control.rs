//! `office.scribe-control` through the built binary, against a temporary
//! workspace (STD-04). Records live in `plugin_state/scribe_control.sqlite`
//! and persist across separate backend processes, which is how the web
//! window and the plugin's hooks share one switch.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_office");

struct Env {
    _dir: tempfile::TempDir,
    workspace: PathBuf,
    state: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = dir.path().join("plugin_state");
        Self {
            _dir: dir,
            workspace,
            state,
        }
    }

    fn library_head(&self) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(self.workspace.join("library"))
            .env_clear()
            .env("PATH", std::env::var("PATH").expect("PATH"))
            .env("HOME", self.workspace.join("home"))
            .output()
            .expect("git");
        assert!(out.status.success());
        String::from_utf8(out.stdout).expect("utf-8")
    }

    fn tool(&self, input: Value) -> Value {
        self.call("office.scribe-control", input)
    }

    fn call(&self, tool: &str, input: Value) -> Value {
        let request = json!({
            "schema_version": 1,
            "tool": tool,
            "input": input,
            "context": {"workspace_root": self.workspace, "plugin_state": self.state},
        })
        .to_string();
        let mut child = Command::new(BIN)
            .args(["orbit-tool"])
            .env_clear()
            .env("HOME", self.workspace.join("home"))
            .env("PATH", std::env::var("PATH").expect("PATH"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(request.as_bytes())
            .expect("write request");
        let out = child.wait_with_output().expect("wait");
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("one JSON document")
    }
}

#[test]
fn records_persist_in_plugin_state_across_backend_processes() {
    let env = Env::new();
    assert_eq!(env.call("office.init", json!({}))["ok"], true);
    let head = env.library_head();
    let sid = "session-one";
    let fresh = env.tool(json!({"op":"get","session_id":sid}));
    assert_eq!(fresh["ok"], true, "{fresh}");
    assert_eq!(fresh["output"]["on"], Value::Null);

    for input in [
        json!({"op":"set_switch","session_id":sid,"on":true}),
        json!({"op":"request_now","session_id":sid}),
        json!({"op":"set_title","session_id":sid,"title":"Release planning"}),
        json!({"op":"heartbeat","session_id":sid}),
    ] {
        let response = env.tool(input);
        assert_eq!(response["ok"], true, "{response}");
    }
    let read = env.tool(json!({"op":"get","session_id":sid}));
    let out = &read["output"];
    assert_eq!(out["on"], true);
    assert_eq!(out["now_requested"], true);
    assert_eq!(out["title"], "Release planning");
    assert_eq!(out["window_open"], true);

    let consumed = env
        .tool(json!({"op":"consume_now","session_id":sid,"requested_at":out["now_requested_at"]}));
    assert_eq!(consumed["output"]["consumed"], true, "{consumed}");
    let again = env.tool(json!({"op":"consume_now","session_id":sid}));
    assert_eq!(again["output"]["consumed"], false);

    assert!(env.state.join("scribe_control.sqlite").is_file());
    assert_eq!(
        env.library_head(),
        head,
        "plugin state only: no Library commit"
    );

    let refused = env.tool(json!({"op":"get","session_id":"../x"}));
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "invalid_argument");
}
