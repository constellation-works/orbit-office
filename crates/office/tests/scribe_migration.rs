//! Scribe v2 S9 migration through the built binary (STD-04):
//! `office.scribe-migrate` (no v1 backfill, the open v1 pending lines
//! reported, the rules-page Tags removal proposed once), the v1
//! `office.scribe-query` modes' `deprecated` answers, and
//! `/scribe note #…` read as a marker. Orbit is a stateful
//! stub on `ORBIT_BIN`; temp roots only, with `HOME` and `TZ` set on the
//! child.
#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SESSION: &str = "11111111-0000-4000-8000-000000000000";
const V2_SESSION: &str = "22222222-0000-4000-8000-000000000000";
const CWD: &str = "/work/orbit-office";

/// A stateful Orbit: proposals filed, listed by tag, shown, updated, and
/// their `proposal.json` artifact stored with its SHA-256.
const ORBIT_STUB: &str = r#"#!/usr/bin/env python3
import hashlib, json, os, sys
args = sys.argv[1:]
assert args[:2] == ['tool', 'run'], args
name, request = args[2], json.loads(args[-1])
state_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'orbit-state.json')
state = json.load(open(state_path)) if os.path.exists(state_path) else {'tasks': []}
def find(task_id):
    return next(t for t in state['tasks'] if t['id'] == task_id)
if name == 'orbit.task.add':
    task = {'id': 'T-%d' % (len(state['tasks']) + 1), 'status': 'proposed',
            'title': request['title'], 'description': request['description'],
            'tags': request.get('tags', [])}
    state['tasks'].append(task)
    result = {'task': task}
elif name == 'orbit.task.list':
    tasks = [t for t in state['tasks']
             if (not request.get('tag') or request['tag'] in t['tags'])
             and (not request.get('status') or t['status'] == request['status'])]
    result = {'tasks': tasks, 'total': len(tasks), 'truncated': False}
elif name == 'orbit.task.show':
    result = {'task': find(request['id'])}
elif name == 'orbit.task.update':
    task = find(request['id'])
    if 'status' in request:
        task['status'] = request['status']
    result = {'task': task}
elif name == 'orbit.task.artifact.put':
    content = open(request['source_path']).read()
    task = find(request['id'])
    task['payload'] = content
    task['sha'] = hashlib.sha256(content.encode()).hexdigest()
    result = {'id': request['id'], 'path': request['path']}
elif name == 'orbit.task.artifact.get':
    result = {'content': find(request['id'])['payload']}
else:
    raise AssertionError(args)
json.dump(state, open(state_path, 'w'))
print(json.dumps(result))
"#;

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(fixture.workspace()).unwrap();
        let stub = fixture.stub();
        std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
        std::fs::write(&stub, ORBIT_STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        fixture.ok("office.init", json!({}));
        fixture
    }

    fn stub(&self) -> PathBuf {
        self.root.path().join("orbit").join("orbit-stub")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("workspace")
    }

    fn call(&self, tool: &str, input: Value) -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", std::env::var("PATH").unwrap())
            .env("TZ", "UTC")
            .env("ORBIT_BIN", self.stub())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"schema_version":1,"tool":tool,"input":input,"context":{
            "workspace_root":self.workspace(),"plugin_state":self.root.path().join("state"),
            "config":{"library_dir":"library"}}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ok(&self, tool: &str, input: Value) -> Value {
        let response = self.call(tool, input);
        assert_eq!(response["ok"], true, "{response}");
        response["output"].clone()
    }

    fn orbit(&self) -> Value {
        let path = self.root.path().join("orbit").join("orbit-state.json");
        std::fs::read_to_string(path).map_or_else(
            |_| json!({"tasks": []}),
            |text| serde_json::from_str(&text).unwrap(),
        )
    }

    fn transcript(&self, session: &str, turns: &[(&str, &str)]) -> PathBuf {
        let lines: Vec<String> = turns
            .iter()
            .enumerate()
            .map(|(n, (role, text))| {
                json!({"type":role,"sessionId":session,
                    "timestamp":format!("2026-10-30T10:{n:02}:00Z"),
                    "cwd":CWD,"message":{"role":role,"content":text}})
                .to_string()
            })
            .collect();
        let path = self.workspace().join(format!("{session}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    /// A v1 scribe pass: a v1 SCR with decision, preference and pending
    /// lines, and a conversation log with a direction and a decision.
    fn v1_history(&self) {
        // v1 preference matters come from the rules page (no Tags section,
        // so the migration proposes no rules change).
        self.ok(
            "office.knowledge-write",
            json!({"markdown":"---\nid: dom-scribe-rules\ntitle: Scribe rules\ntemplate: scribe-rules\n---\n## Matters\n- merge-style\n- reply-style\n"}),
        );
        let transcript = self.transcript(
            SESSION,
            &[
                (
                    "user",
                    "Decision: keep the v1 readers because they are evidence.",
                ),
                ("user", "Preference: always squash merge."),
                ("user", "Preference: rebase merges are fine now."),
                ("assistant", "Pending: waiting on the CI run."),
                ("user", "Preference: answer in short paragraphs."),
            ],
        );
        self.ok(
            "office.scribe-pending",
            json!({"session_id":SESSION,"transcript":transcript,"provider":"claude-code","cwd":CWD}),
        );
        let appended = self.ok(
            "office.scribe-append",
            json!({"session_id":SESSION,"through":"m-0005","title":"Keep the v1 readers","entries":[
                {"msg":"m-0001","tag":"decision","quote":"keep the v1 readers",
                 "why":{"msg":"m-0001","quote":"they are evidence"}},
                {"msg":"m-0002","tag":"preference","quote":"always squash merge","matter":"merge-style"},
                {"msg":"m-0003","tag":"preference","quote":"rebase merges are fine now","matter":"merge-style"},
                {"msg":"m-0004","tag":"pending","quote":"waiting on the CI run","by":"orchestrator"},
                {"msg":"m-0005","tag":"preference","quote":"answer in short paragraphs","matter":"reply-style"}]}),
        );
        assert_eq!(appended["written"], true, "{appended}");
        let captured = self.ok(
            "office.capture",
            json!({"kind":"conversation","title":"Release planning","tags":["project:orbit-office"],"entries":[
                {"kind":"direction","text":"Never deploy on Fridays."},
                {"kind":"decision","text":"Ship the migration first.","by":"user"}]}),
        );
        assert!(
            captured["id"].as_str().unwrap().starts_with("src-"),
            "{captured}"
        );
    }
}

fn proposals(state: &Value, kind: &str) -> Vec<Value> {
    state["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            t["tags"]
                .as_array()
                .unwrap()
                .contains(&json!(format!("proposal:{kind}")))
        })
        .cloned()
        .collect()
}

#[test]
fn migrate_backfills_nothing_and_reports_open_pending_lines() {
    let fixture = Fixture::new();
    fixture.v1_history();

    // v2 starts clean: v1 preferences, directions and decisions are not
    // proposed, and the open v1 pending line is reported, not filed.
    let out = fixture.ok("office.scribe-migrate", json!({}));
    assert_eq!(out["proposals"], json!([]), "{out}");
    assert_eq!(out["pending"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(out["pending"][0]["quote"], "waiting on the CI run");
    assert!(
        out["pending"][0]["cite"].as_str().unwrap().contains("#L"),
        "{out}"
    );
    assert_eq!(fixture.orbit()["tasks"], json!([]));
}

#[test]
fn the_rules_page_tags_section_is_proposed_for_removal_once() {
    let fixture = Fixture::new();
    let page = "---\nid: dom-scribe-rules\ntitle: Scribe rules\ntemplate: scribe-rules\nscribe:\n  handoff: true\n---\n## Tags\n- decision: Daniel settles a choice.\n\n## Topics\n- orbit-office\n";
    let written = fixture.ok("office.knowledge-write", json!({"markdown":page}));
    assert_eq!(written["id"], "dom-scribe-rules", "{written}");
    let out = fixture.ok("office.scribe-migrate", json!({}));
    assert_eq!(out["proposals"][0]["key"], "rules", "{out}");
    let state = fixture.orbit();
    let task = proposals(&state, "knowledge_update").remove(0);
    let payload: Value = serde_json::from_str(task["payload"].as_str().unwrap()).unwrap();
    let markdown = payload["markdown"].as_str().unwrap();
    assert!(!markdown.contains("## Tags"), "{markdown}");
    assert!(!markdown.contains("handoff"), "{markdown}");
    assert!(markdown.contains("## Topics\n- orbit-office"), "{markdown}");
    // Accepting it applies; a re-run then has nothing to file.
    fixture.ok(
        "office.proposal-accept",
        json!({"id": task["id"], "sha256": task["sha"]}),
    );
    let again = fixture.ok("office.scribe-migrate", json!({}));
    assert_eq!(again["proposals"].as_array().unwrap().len(), 0, "{again}");
}

#[test]
fn v1_query_modes_answer_deprecated_with_the_replacement_and_v2_first() {
    let fixture = Fixture::new();
    fixture.v1_history();
    for mode in ["pending", "landed", "pref", "why"] {
        let out = fixture.ok("office.scribe-query", json!({"mode": mode}));
        assert_eq!(out["deprecated"], true, "{mode}: {out}");
        assert!(out["replacement"].as_str().unwrap().len() > 10, "{out}");
    }
    let pref = fixture.ok(
        "office.scribe-query",
        json!({"mode":"pref","matter":"merge-style"}),
    );
    assert_eq!(
        pref["results"][0]["quote"], "rebase merges are fine now",
        "{pref}"
    );
    assert_eq!(pref["v2_results"], json!([]), "{pref}");
}

#[test]
fn a_scribe_note_command_with_a_tag_is_read_as_that_marker() {
    let fixture = Fixture::new();
    let transcript = fixture.transcript(
        V2_SESSION,
        &[
            ("user", "/office-scribe:scribe note #decision ship v2 first"),
            ("user", "/scribe note untagged text"),
        ],
    );
    let pending = fixture.ok(
        "office.scribe-pending",
        json!({"session_id":V2_SESSION,"transcript":transcript,"provider":"claude-code",
            "cwd":CWD,"format":"v2"}),
    );
    // The tagged note is a marker on its message; the untagged one is not.
    let markers = pending["markers"].as_array().unwrap();
    assert_eq!(markers.len(), 1, "{pending}");
    assert_eq!(markers[0]["msg"], "m-0001");
    assert_eq!(markers[0]["tag"], "decisions", "{pending}");
    assert_eq!(markers[0]["by"], "user");
}

/// The v1 branch of the shipped `scribe-query` response schema.
fn v1_response_schema() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/schemas/scribe-query.response.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    schema["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["title"] == "v1")
        .unwrap()
        .clone()
}

/// The schema's closed v1 branch admits every v1-mode response the binary
/// returns, including the window's `deprecated`, `replacement` and
/// `v2_results` fields, and their declared types hold.
#[test]
fn v1_mode_responses_conform_to_the_published_response_schema() {
    let fixture = Fixture::new();
    fixture.v1_history();
    let branch = v1_response_schema();
    assert_eq!(branch["additionalProperties"], false);
    let properties = branch["properties"].as_object().unwrap();
    for input in [
        json!({"mode":"pending"}),
        json!({"mode":"landed"}),
        json!({"mode":"pref","matter":"merge-style"}),
        json!({"mode":"why","matter":"v1 readers"}),
        json!({"tag":"decision"}),
    ] {
        let out = fixture.ok("office.scribe-query", input.clone());
        for key in out.as_object().unwrap().keys() {
            assert!(properties.contains_key(key), "{input}: undeclared {key}");
        }
        for key in branch["required"].as_array().unwrap() {
            assert!(
                out.get(key.as_str().unwrap()).is_some(),
                "{input}: missing {key}"
            );
        }
        if input.get("mode").is_some() {
            assert_eq!(
                out["deprecated"], properties["deprecated"]["const"],
                "{input}"
            );
            assert!(out["replacement"].is_string(), "{input}");
        } else {
            assert!(
                out.get("deprecated").is_none(),
                "the general query is not deprecated"
            );
        }
        if let Some(items) = out.get("v2_results") {
            assert_eq!(properties["v2_results"]["type"], "array");
            assert!(
                items.as_array().unwrap().iter().all(Value::is_object),
                "{out}"
            );
        }
    }
}

/// The tag definitions in a tool response, and the closed item schema the
/// shipped response schema declares for them.
fn tag_item_properties(schema_file: &str, pointer: &str) -> serde_json::Map<String, Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/schemas")
        .join(schema_file);
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let items = schema.pointer(pointer).unwrap();
    assert_eq!(items["additionalProperties"], false, "{schema_file}");
    items["properties"].as_object().unwrap().clone()
}

/// Every key a tag definition carries in `office.status` and
/// `office.scribe-pending` is declared by the closed tag schema, so Orbit's
/// output validation admits the response.
#[test]
fn tag_definitions_conform_to_the_published_response_schemas() {
    let fixture = Fixture::new();
    let status = fixture.ok("office.status", json!({}));
    let declared = tag_item_properties(
        "status.response.json",
        "/properties/scribe/properties/tags/items",
    );
    let tags = status["scribe"]["tags"].as_array().unwrap();
    assert!(!tags.is_empty(), "{status}");
    for tag in tags {
        for key in tag.as_object().unwrap().keys() {
            assert!(declared.contains_key(key), "status: undeclared {key}");
        }
    }
}
