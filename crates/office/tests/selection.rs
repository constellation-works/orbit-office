//! Selection tools through the built binary, against a temporary workspace
//! (STD-04). The tray file is `plugin_state/selections.sqlite`, beside and
//! distinct from the Library index.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

    fn tool(&self, tool: &str, input: serde_json::Value) -> serde_json::Value {
        let path = std::env::var("PATH").expect("PATH");
        let request = serde_json::json!({
            "schema_version": 1,
            "tool": tool,
            "input": input,
            "context": {
                "workspace_root": self.workspace,
                "plugin_state": self.state,
            },
        })
        .to_string();
        let mut child = Command::new(BIN)
            .args(["orbit-tool"])
            .env_clear()
            .env("HOME", self.workspace.join("home"))
            .env("PATH", path)
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
fn selection_put_and_get_use_a_tray_beside_the_index() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", serde_json::json!({}))["ok"], true);

    let recorded = env.tool(
        "office.selection-put",
        serde_json::json!({
            "object": "src-a",
            "view": "source",
            "anchor": "#L1",
            "quote": "alpha"
        }),
    );
    assert_eq!(recorded["ok"], true, "{recorded}");
    assert_eq!(recorded["output"]["selection"]["object"], "src-a");
    assert_eq!(recorded["output"]["selection"]["quote"], "alpha");
    assert!(recorded["output"]["selection"]["quote_sha256"].is_string());
    assert!(recorded["output"]["selection"]["selected_at"].is_string());

    let index = env.state.join("index.sqlite");
    let tray = env.state.join("selections.sqlite");
    assert!(index.is_file(), "index exists");
    assert!(tray.is_file(), "selections database exists");
    assert_ne!(index, tray);

    let peek = env.tool("office.selection-get", serde_json::json!({"peek": true}));
    assert_eq!(peek["ok"], true, "{peek}");
    assert_eq!(peek["output"]["selections"][0]["quote"], "alpha");

    let first = env.tool("office.selection-get", serde_json::json!({}));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(
        first["output"]["selections"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(first["output"]["selections"][0]["quote"], "alpha");
    let second = env.tool("office.selection-get", serde_json::json!({}));
    assert_eq!(second["output"]["selections"], serde_json::json!([]));

    let private = env.tool(
        "office.selection-put",
        serde_json::json!({
            "object": "src-private",
            "view": "page",
            "quote": "secret words",
            "quote_sha256": "not-stored",
            "sensitivity": "private"
        }),
    );
    assert_eq!(private["ok"], true, "{private}");
    assert!(private["output"]["selection"]["quote"].is_null());
    assert!(private["output"]["selection"]["quote_sha256"].is_null());
    let private_got = env.tool("office.selection-get", serde_json::json!({}));
    assert!(private_got["output"]["selections"][0]["quote"].is_null());
    assert!(private_got["output"]["selections"][0]["quote_sha256"].is_null());

    let cleared = env.tool("office.selection-put", serde_json::json!({"clear": true}));
    assert_eq!(
        cleared["output"],
        serde_json::json!({"cleared": true, "evicted": [], "selection": null})
    );
    assert_eq!(
        env.tool("office.selection-get", serde_json::json!({}))["output"]["selections"],
        serde_json::json!([])
    );

    let mut first_id = String::new();
    for index in 1..=6 {
        let response = env.tool(
            "office.selection-put",
            serde_json::json!({
                "object": format!("obj-{index}"),
                "view": "artifact",
                "quote": "q"
            }),
        );
        assert_eq!(response["ok"], true, "{response}");
        if index == 1 {
            first_id = response["output"]["selection"]["id"]
                .as_str()
                .expect("id")
                .to_string();
        }
        if index == 6 {
            assert_eq!(response["output"]["evicted"], serde_json::json!([first_id]));
        }
    }
    let held = env.tool("office.selection-get", serde_json::json!({}));
    let ids: Vec<&str> = held["output"]["selections"]
        .as_array()
        .expect("selections")
        .iter()
        .map(|selection| selection["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids.len(), 5);
    assert!(!ids.contains(&first_id.as_str()));
    assert_eq!(
        env.tool("office.selection-get", serde_json::json!({}))["output"]["selections"],
        serde_json::json!([])
    );

    let missing_view = env.tool(
        "office.selection-put",
        serde_json::json!({"object": "src-a"}),
    );
    assert_eq!(missing_view["ok"], false, "{missing_view}");
    assert_eq!(missing_view["error"]["code"], "invalid_argument");
    let unknown = env.tool("office.selection-get", serde_json::json!({"verbose": true}));
    assert_eq!(unknown["error"]["code"], "invalid_argument");
}
