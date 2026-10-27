//! The public adapter against a stub of Orbit's CLI callback channel.
#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use office_core::ports::Orbit;
use office_orbit::{Error, ORBIT_TOOLS, OrbitCallbacks};
use serde_json::{Map, Value, json};

struct Stub {
    root: tempfile::TempDir,
    program: PathBuf,
}

impl Stub {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let program = root.path().join("orbit-stub");
        fs::write(
            &program,
            r#"#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
assert args[:2] == ['tool', 'run']
assert args[3:6] == ['--json', '--full', '--input']
assert len(args) == 7
input = json.loads(args[6])
receipt = {'tool': args[2], 'input': input, 'cwd': os.getcwd(),
           'home': os.environ.get('HOME'),
           'marker': os.environ.get('ORBIT_PLUGIN_CALLBACK'),
           'forbidden': [key for key in ['ORBIT_OPERATOR', 'ORBIT_WORKSPACE_CLAIM_TOKEN',
                                        'META_API_KEY', 'UNRELATED'] if key in os.environ]}
previous = json.loads(Path('receipt.json').read_text()) if Path('receipt.json').exists() else {}
receipt['invocations'] = previous.get('invocations', 0) + 1
fd = os.environ.get('ORBIT_PLUGIN_CALLBACK_FD')
if fd:
    receipt['credential'] = os.pread(int(fd), 1024, 0).decode()
Path('receipt.json').write_text(json.dumps(receipt))
response = json.loads(Path('response.json').read_text())
sys.stdout.write(response.get('stdout', ''))
sys.stderr.write(response.get('stderr', ''))
sys.exit(response.get('exit', 0))
"#,
        )
        .expect("stub program");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).expect("executable");
        Self { root, program }
    }

    fn adapter(&self) -> OrbitCallbacks {
        self.adapter_with(Vec::new())
    }

    fn adapter_with(&self, mut environment: Vec<(OsString, OsString)>) -> OrbitCallbacks {
        environment.extend([
            ("HOME".into(), self.root.path().as_os_str().to_owned()),
            (
                "PATH".into(),
                std::env::var_os("PATH").expect("python3 on fixture PATH"),
            ),
            (
                "ORBIT_ROOT".into(),
                self.root.path().join("orbit").into_os_string(),
            ),
            ("ORBIT_OPERATOR".into(), "1".into()),
            ("ORBIT_WORKSPACE_CLAIM_TOKEN".into(), "fixture-claim".into()),
            ("META_API_KEY".into(), "fixture-key".into()),
            ("UNRELATED".into(), "value".into()),
        ]);
        OrbitCallbacks::new(&self.program, self.root.path(), environment)
    }

    fn response(&self, output: &Value) {
        self.raw_response(json!({"stdout": output.to_string()}));
    }

    fn raw_response(&self, response: Value) {
        fs::write(self.root.path().join("response.json"), response.to_string()).unwrap();
    }

    fn receipt(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.root.path().join("receipt.json")).unwrap())
            .unwrap()
    }
}

type Method = fn(&dyn Orbit, &Map<String, Value>) -> office_core::Result<Value>;

fn callbacks() -> [(&'static str, Method, Value, Value); 8] {
    [
        (
            "orbit.task.add",
            |p, i| p.task_add(i),
            json!({"title":"A quote ' and `literal` $(literal)\nsecond line", "tags":["office"]}),
            json!({"id":"<id>","title":"task"}),
        ),
        (
            "orbit.task.update",
            |p, i| p.task_update(i),
            json!({"id":"<id>","comment":"filed", "unknown_future_key": true}),
            json!({"id":"<id>","status":"backlog"}),
        ),
        (
            "orbit.task.list",
            |p, i| p.task_list(i),
            json!({"limit":1}),
            json!({"tasks":[{"id":"<id>"}],"total":2,"truncated":true}),
        ),
        (
            "orbit.task.show",
            |p, i| p.task_show(i),
            json!({"id":"<id>","field":"comments"}),
            json!([{"message":"projection"}]),
        ),
        (
            "orbit.task.artifact.get",
            |p, i| p.task_artifact_get(i),
            json!({"id":"<id>","path":"output/manifest.json"}),
            json!({"content":"{\n  \"files\": []\n}","path":"output/manifest.json"}),
        ),
        (
            "orbit.task.artifact.put",
            |p, i| p.task_artifact_put(i),
            json!({"id":"<id>","source_path":".orbit/tmp/output.json","path":"output.json"}),
            json!({"path":"output.json","size_bytes":12}),
        ),
        (
            "orbit.friction.add",
            |p, i| p.friction_add(i),
            json!({"title":"finding","description":"evidence"}),
            json!({"id":"<id>","title":"finding"}),
        ),
        (
            "orbit.search",
            |p, i| p.search(i),
            json!({"query":"office", "kind":"task"}),
            json!({"results":[],"total":0,"truncated":false}),
        ),
    ]
}

#[test]
fn all_eight_port_methods_use_the_callback_channel_and_preserve_payloads() {
    let stub = Stub::new();
    let adapter = stub.adapter();
    for (name, method, input, output) in callbacks() {
        stub.response(&output);
        assert_eq!(
            method(&adapter, input.as_object().unwrap()).unwrap(),
            output,
            "{name}"
        );
        let receipt = stub.receipt();
        assert_eq!(receipt["tool"], name);
        assert_eq!(receipt["input"], input);
        assert_eq!(
            receipt["cwd"],
            stub.root.path().canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(receipt["home"], stub.root.path().to_str().unwrap());
        assert_eq!(receipt["forbidden"], json!([]));
        assert_eq!(receipt["marker"], "descriptor-required");
    }
    assert_eq!(ORBIT_TOOLS, callbacks().map(|(name, _, _, _)| name));
}

#[test]
fn every_unlisted_name_is_refused_before_starting_the_channel() {
    let stub = Stub::new();
    let adapter = stub.adapter();
    for name in [
        "orbit.task.reject",
        "orbit.task.add.extra",
        "orbit.*",
        "office.status",
        "",
        "--help",
    ] {
        assert!(matches!(
            adapter.call(name, &Map::new()),
            Err(Error::ToolNotAllowed)
        ));
    }
    assert!(!stub.root.path().join("receipt.json").exists());
}

#[test]
fn orbit_refusals_are_typed_for_every_callback_and_discard_raw_diagnostics() {
    let stub = Stub::new();
    let adapter = stub.adapter();
    for code in [
        "policy_denied",
        "capability_refused",
        "task_not_found",
        "invalid_input",
    ] {
        stub.raw_response(json!({"exit":1,"stderr":json!({"code":code,"error":"fixture-secret", "detail":"fixture-secret"}).to_string()}));
        for (_, method, input, _) in callbacks() {
            let error = method(&adapter, input.as_object().unwrap()).unwrap_err();
            assert!(
                matches!(&error, office_core::Error::OrbitRefused { code: actual } if actual == code)
            );
            assert!(!error.to_string().contains("fixture-secret"));
        }
    }
}

#[test]
fn invalid_json_and_unstructured_failure_are_distinct_and_do_not_leak_output() {
    let stub = Stub::new();
    let adapter = stub.adapter();
    stub.raw_response(json!({"stdout":"fixture-secret"}));
    assert!(matches!(
        adapter.call("orbit.search", &Map::new()),
        Err(Error::InvalidResponse)
    ));
    for stderr in [
        "fixture-secret",
        r#"{"code":"policy_denied"}"#,
        r#"{"code":"SECRET!","error":"fixture-secret"}"#,
    ] {
        stub.raw_response(json!({"exit":7,"stderr":stderr}));
        let error = adapter.call("orbit.search", &Map::new()).unwrap_err();
        assert!(matches!(error, Error::CommandFailed { exit_code: Some(7) }));
        assert!(!error.to_string().contains("fixture-secret"));
    }
    let missing = OrbitCallbacks::new(stub.root.path().join("missing"), stub.root.path(), []);
    assert!(matches!(
        missing.call("orbit.search", &Map::new()),
        Err(Error::Process(_))
    ));
}

#[test]
fn lost_write_replies_are_outcome_unknown_and_are_never_retried() {
    let stub = Stub::new();
    let adapter = stub.adapter();
    let mut invocations = 0;
    for response in [
        json!({"stdout":"incomplete"}),
        json!({"exit":1,"stderr":"unstructured"}),
        json!({"exit":1,"stderr":json!({"code":"outcome_unknown","error":"write reply was lost"}).to_string()}),
    ] {
        stub.raw_response(response);
        for (name, method, input, _) in callbacks() {
            if [
                "orbit.task.add",
                "orbit.task.update",
                "orbit.task.artifact.put",
                "orbit.friction.add",
            ]
            .contains(&name)
            {
                assert!(matches!(
                    method(&adapter, input.as_object().unwrap()),
                    Err(office_core::Error::OrbitOutcomeUnknown)
                ));
                invocations += 1;
                assert_eq!(stub.receipt()["invocations"], invocations);
            }
        }
    }
    let missing = OrbitCallbacks::new(stub.root.path().join("missing"), stub.root.path(), []);
    // A failed spawn proves no write was sent, rather than an unknown outcome.
    assert!(matches!(
        missing.task_add(&Map::new()),
        Err(office_core::Error::Orbit {
            code: "orbit_transport_failed",
            ..
        })
    ));
}

#[test]
fn callback_descriptor_is_inherited_without_becoming_tool_input() {
    use std::os::fd::{AsRawFd, FromRawFd};
    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
    }
    let stub = Stub::new();
    let credential = tempfile::tempfile().unwrap();
    use std::io::Write;
    (&credential).write_all(b"fixture-credential").unwrap();
    // dup creates a fresh owned descriptor without CLOEXEC. It touches no other
    // test's descriptors and models the host's already-inherited credential.
    let fd = unsafe { dup(credential.as_raw_fd()) };
    assert!(fd > 2);
    let inherited = unsafe { fs::File::from_raw_fd(fd) };
    let adapter = stub.adapter_with(vec![
        ("ORBIT_PLUGIN_CALLBACK_FD".into(), fd.to_string().into()),
        ("ORBIT_PLUGIN_CALLBACK".into(), "descriptor-required".into()),
    ]);
    stub.response(&json!({"ok":true}));
    adapter.task_show(&Map::new()).unwrap();
    let receipt = stub.receipt();
    assert_eq!(receipt["credential"], "fixture-credential");
    assert_eq!(receipt["marker"], "descriptor-required");
    assert_eq!(receipt["input"], json!({}));
    drop(inherited);
}
