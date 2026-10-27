//! Built binary: composition, approval callbacks, preflight refusals and filing.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::domain::artifact::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Stdio},
};
struct Fixture {
    root: tempfile::TempDir,
    script: PathBuf,
    input: Value,
}
impl Fixture {
    fn new(kind: ArtifactKind) -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("uv"),r#"#!/usr/bin/python3 -S
import os,sys,json,pathlib
p=pathlib.Path
assert '--offline' in sys.argv and '--locked' in sys.argv
p('tmp/record.json').write_text(json.dumps({'env':dict(os.environ),'argv':sys.argv[1:],'cwd':os.getcwd()}))
if sys.argv[-1]=='outputs': p('out/result.txt').write_text('result')
print('done')
"#).unwrap();
        fs::write(bin.join("orbit"),r#"#!/usr/bin/python3 -S
import sys,json,pathlib
p=pathlib.Path
request=json.loads(sys.argv[-1]); tool=sys.argv[3]
with p('calls.jsonl').open('a') as calls: calls.write(json.dumps({'tool':tool,'input':request})+'\n')
if tool=='orbit.task.list':
    result={'tasks':[] if p('unapproved').exists() else [{'id':'proposal','status':'done','tags':['proposal','proposal:routine']}],'truncated':False}
elif tool=='orbit.task.artifact.get':
    result={'content':json.dumps({'kind':'routine','definition':{'steps':[{'kind':'plugin.tool_call','tool':'office.script-run','input':json.loads(p('input.json').read_text())}]}})}
elif tool=='orbit.task.add':
    assert 'status' not in request
    assert request['complexity']=='low'
    result={'id':'output-task'}
elif tool=='orbit.task.artifact.put':
    path=p(request['source_path']); assert path.resolve().is_relative_to(p.cwd())
    p('attached-'+request['path']).write_bytes(path.read_bytes()); result={}
else: raise AssertionError(tool)
print(json.dumps(result))
"#).unwrap();
        for name in ["uv", "orbit"] {
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let artifact = Artifact::approved(
            ArtifactManifest {
                kind,
                title: "Fixture script".into(),
                summary: String::new(),
                files: vec!["script.py".into()],
                cites: vec![],
                series: "script".into(),
                tags: vec![],
                runbook: None,
                sensitivity: Default::default(),
            },
            "fixture".into(),
            Producer {
                crew: None,
                runbook: None,
                at: None,
            },
            Approval {
                by: Approver::User,
                at: "1970-01-01T00:00:00Z".into(),
                edited: false,
            },
        )
        .unwrap();
        let input = json!({"artifact":artifact.id,"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","args":[],"inputs":[],"output":{"title":"Output","kind":"data","series":"output"}});
        let script = root
            .path()
            .join("library")
            .join(artifact.path().unwrap())
            .with_file_name("script.py");
        let f = Self {
            root,
            script,
            input,
        };
        assert_eq!(f.call("office.init", json!({}))["ok"], true);
        fs::create_dir_all(f.script.parent().unwrap()).unwrap();
        fs::write(
            f.script.with_file_name("artifact.md"),
            artifact.to_markdown().unwrap(),
        )
        .unwrap();
        fs::write(&f.script, "").unwrap();
        let prepared = f
            .root
            .path()
            .join("state/uv-cache/prepared")
            .join(f.input["sha256"].as_str().unwrap());
        fs::create_dir_all(&prepared).unwrap();
        fs::write(prepared.join("script.py.lock"), "lock").unwrap();
        f
    }
    fn call(&self, tool: &str, input: Value) -> Value {
        if tool == "office.script-run" {
            fs::write(self.root.path().join("input.json"), input.to_string()).unwrap();
        }
        let envelope = json!({"schema_version":1,"tool":tool,"input":input,"context":{"workspace_root":self.root.path(),"plugin_state":self.root.path().join("state"),"secrets":{"meta_api_key":"never-pass-context-secret"}}});
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .env("LANG", "C.UTF-8")
            .env("TZ", "UTC")
            .env("ORBIT_BIN", self.root.path().join("bin/orbit"))
            .env("ORBIT_RUN_ID", "jrun-20261027-0517-abc")
            .env("AMBIENT_SECRET", "never-pass-ambient")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(envelope.to_string().as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    }
    fn no_runs(&self) {
        assert!(!self.root.path().join("state/runs").exists());
    }
}
#[test]
fn binary_refuses_unapproved_kind_proposal_digest_and_symlink_input_before_creating_runs() {
    let f = Fixture::new(ArtifactKind::Data);
    assert_eq!(
        f.call("office.script-run", f.input.clone())["error"]["code"],
        "script_not_approved"
    );
    f.no_runs();
    let f = Fixture::new(ArtifactKind::Script);
    fs::write(f.root.path().join("unapproved"), "").unwrap();
    assert_eq!(
        f.call("office.script-run", f.input.clone())["error"]["code"],
        "script_not_approved"
    );
    f.no_runs();
    let f = Fixture::new(ArtifactKind::Script);
    fs::write(&f.script, "changed").unwrap();
    assert_eq!(
        f.call("office.script-run", f.input.clone())["error"]["code"],
        "script_digest_mismatch"
    );
    f.no_runs();
    let mut f = Fixture::new(ArtifactKind::Script);
    fs::write(f.root.path().join("outside"), "private").unwrap();
    symlink(
        f.root.path().join("outside"),
        f.root.path().join("library/link"),
    )
    .unwrap();
    for input in ["../outside", "link"] {
        f.input["inputs"] = json!([input]);
        assert_eq!(
            f.call("office.script-run", f.input.clone())["error"]["code"],
            "input_outside_library"
        );
        f.no_runs();
    }
}
#[test]
fn binary_composes_safe_runner_and_files_one_task_with_outputs_and_manifests() {
    let mut f = Fixture::new(ArtifactKind::Script);
    f.input["args"] = json!(["outputs"]);
    let response = f.call("office.script-run", f.input.clone());
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["output_task"], "output-task");
    let dir = PathBuf::from(response["output"]["run_dir"].as_str().unwrap());
    assert!(
        dir.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("jrun-20261027-0517-abc-")
    );
    let record: Value =
        serde_json::from_slice(&fs::read(dir.join("tmp/record.json")).unwrap()).unwrap();
    let mut env = record["env"].as_object().unwrap().clone();
    // The fake `uv` runs under `/usr/bin/python3`, which on macOS is the
    // Xcode shim: it injects these keys after the runner's cleared env, so
    // they are not something the runner passed.
    if cfg!(target_os = "macos") {
        for key in [
            "CPATH",
            "LC_CTYPE",
            "LIBRARY_PATH",
            "MANPATH",
            "SDKROOT",
            "__CF_USER_TEXT_ENCODING",
        ] {
            env.remove(key);
        }
    }
    assert_eq!(env.len(), 7, "{env:?}");
    assert!(!env.contains_key("AMBIENT_SECRET"));
    assert!(!env.contains_key("ORBIT_BIN"));
    assert!(!env.contains_key("ORBIT_RUN_ID"));
    assert!(!record.to_string().contains("never-pass"));
    assert_eq!(
        fs::read_to_string(f.root.path().join("attached-result.txt")).unwrap(),
        "result"
    );
    let calls = fs::read_to_string(f.root.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["tool"] == "orbit.task.add")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["tool"] == "orbit.task.artifact.put")
            .count(),
        3
    );
    assert_eq!(
        calls.last().unwrap()["input"]["path"],
        "artifact-manifest.json"
    );
    assert!(
        fs::read_dir(f.root.path().join("library/.office/attach"))
            .unwrap()
            .next()
            .is_none(),
        "staging is removed after transfer"
    );
}
