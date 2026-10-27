//! Through the built binary, real Library and stubbed Orbit callback channel.
#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]
use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    workspace: PathBuf,
    callback: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let callback = root.path().join("orbit-fixture");
        std::fs::write(&callback,r#"#!/usr/bin/env python3
import base64, json, sys
args=sys.argv[1:]
assert args[:2] == ['tool', 'run']
request=json.loads(args[-1])
if args[2] == 'orbit.task.show':
    result={'id':request['id'],'status':'review','crew':'fixture-crew','updated_at':'2026-10-27T10:00:00Z',
            'artifacts':[] if request['id']=='<empty>' else [{'path':'artifact-manifest.json'},{'path':'data.bin'}]}
elif args[2] == 'orbit.task.artifact.get':
    if request['path']=='artifact-manifest.json':
        result={'content':json.dumps({'kind':'data','title':'CLI output','files':['data.bin'],'series':'cli-output'})}
    else:
        data=bytes(range(256))*4096
        result={'content_base64':base64.b64encode(data).decode(), 'size':len(data)}
else:
    raise AssertionError(args)
print(json.dumps(result))
"#).unwrap();
        std::fs::set_permissions(&callback, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            workspace,
            callback,
        }
    }
    fn call(&self, tool: &str, input: Value) -> Value {
        let raw = json!({"schema_version":1,"tool":tool,"input":input,"context":{
            "workspace_root":self.workspace,"plugin_state":self.root.path().join("state")}})
        .to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path())
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("ORBIT_BIN", &self.callback)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(raw.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn library(&self) -> PathBuf {
        self.workspace.join("library")
    }
}

#[test]
fn binary_promotes_one_mib_revises_and_shows_files_series_and_citing_pages() {
    let fixture = Fixture::new();
    assert_eq!(fixture.call("office.init", json!({}))["ok"], true);
    let promoted = fixture.call(
        "office.artifact-promote",
        json!({"task":"<id>","edited":true}),
    );
    assert_eq!(promoted["ok"], true, "{promoted}");
    let id = promoted["output"]["id"].as_str().unwrap();
    let path = promoted["output"]["path"].as_str().unwrap();
    let original = std::fs::read(fixture.library().join(path)).unwrap();
    let file = fixture
        .library()
        .join(Path::new(path).parent().unwrap())
        .join("data.bin");
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(bytes.len(), 1_048_576);
    assert!(
        bytes
            .chunks_exact(256)
            .all(|chunk| chunk.iter().copied().eq(0..=255))
    );
    let citing=fixture.call("office.knowledge-write",json!({"markdown":format!("---\nid: dom-result\ntitle: Result\ncites: [{id}]\n---\nApproved findings.\n")}));
    assert_eq!(citing["ok"], true, "{citing}");
    let shown = fixture.call("office.artifact-show", json!({"id":id}));
    assert_eq!(shown["ok"], true, "{shown}");
    assert_eq!(shown["output"]["artifact"]["approved"]["by"], "user");
    assert_eq!(shown["output"]["artifact"]["approved"]["edited"], true);
    assert_eq!(
        shown["output"]["artifact"]["produced_by"]["crew"],
        "fixture-crew"
    );
    assert_eq!(shown["output"]["backlinks"][0]["id"], "dom-result");
    assert_eq!(shown["output"]["files"][0]["name"], "data.bin");
    let revised = fixture.call("office.artifact-promote", json!({"task":"<id>"}));
    assert_eq!(revised["ok"], true, "{revised}");
    assert_eq!(revised["output"]["supersedes"], id);
    assert_eq!(revised["output"]["version"], 2);
    let shown = fixture.call("office.artifact-show", json!({"id":id}));
    assert_eq!(shown["output"]["series_history"][0]["version"], 2);
    assert_eq!(
        shown["output"]["series_history"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        std::fs::read(fixture.library().join(path)).unwrap(),
        original
    );
    let empty = fixture.call("office.artifact-promote", json!({"task":"<empty>"}));
    assert_eq!(empty["error"]["code"], "task_has_no_artifacts");
}
