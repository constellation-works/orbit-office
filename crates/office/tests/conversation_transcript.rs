//! Conversation extraction through the built binary with a contained transcript.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            root: tempfile::tempdir().unwrap(),
        };
        assert_eq!(f.call(json!({}), "office.init")["ok"], true);
        f
    }
    fn workspace(&self) -> PathBuf {
        self.root.path().join("workspace")
    }
    fn transcript(&self) -> PathBuf {
        self.workspace().join("session.jsonl")
    }
    fn call(&self, input: Value, tool: &str) -> Value {
        std::fs::create_dir_all(self.workspace()).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", std::env::var("PATH").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"schema_version":1,"tool":tool,"input":input,"context":{
            "workspace_root":self.workspace(),"plugin_state":self.root.path().join("state")}});
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
    fn input(&self) -> Value {
        json!({"session_transcript_path":self.transcript(),"session_id":"fixture-session"})
    }
    fn write(&self, extra: &str) {
        let records = [
            json!({"type":"user","sessionId":"fixture-session","timestamp":"2026-10-30T10:00:00Z","message":{"role":"user","content":"Always draft client emails, never send.\nMaybe we should switch vendors?\nMy token is sk-test-1234567890abcdefghijklmnopqrstuvwxyz"}}),
            json!({"type":"assistant","sessionId":"fixture-session","timestamp":"2026-10-30T10:01:00Z","message":{"role":"assistant","model":"claude-opus-fixture","content":[{"type":"text","text":"Decision: switch vendors."},{"type":"tool_use","name":"example","input":{"secret":"not included"}}]}}),
        ];
        let mut text = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        if !extra.is_empty() {
            text.push('\n');
            text.push_str(&json!({"type":"user","sessionId":"fixture-session","timestamp":"2026-10-30T10:02:00Z","message":{"role":"user","content":extra}}).to_string());
        }
        std::fs::write(self.transcript(), text).unwrap();
    }
}
#[test]
fn transcript_capture_is_verbatim_guarded_private_versioned_and_idempotent() {
    let f = Fixture::new();
    f.write("");
    let first = f.call(f.input(), "office.capture");
    assert_eq!(first["ok"], true, "{first}");
    let out = &first["output"];
    assert!(
        out["path"]
            .as_str()
            .unwrap()
            .starts_with("sources/conversation/2026/10/")
    );
    assert_eq!(out["sensitivity"], "private");
    let path = f
        .workspace()
        .join("library")
        .join(out["path"].as_str().unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    let meta = office_core::domain::metrics::metadata(&text).unwrap();
    assert_eq!(meta["orchestrator"], "claude");
    assert_eq!(meta["model"], "claude-opus-fixture");
    assert_eq!(meta["session_id"], "fixture-session");
    assert_eq!(meta["captured_by"], "transcript-extractor");
    assert!(text.contains("direction: Always draft client emails, never send."));
    assert!(text.contains("open: Maybe we should switch vendors?"));
    assert!(!text.contains("decision: switch vendors"));
    assert!(!text.contains("sk-test-1234567890abcdefghijklmnopqrstuvwxyz"));
    let again = f.call(f.input(), "office.capture");
    assert_eq!(again["output"]["id"], out["id"]);
    assert_eq!(again["output"]["deduplicated"], true);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let append = f.call(json!({"append_to":out["id"],"entries":[{"kind":"handoff","text":"Next: review the draft. Bearer credential-for-handoff-fixture"}]}), "office.capture");
    assert_eq!(append["ok"], true, "{append}");
    f.write("Decision: keep the current vendor.");
    let changed = f.call(f.input(), "office.capture");
    assert_eq!(changed["ok"], true, "{changed}");
    assert_eq!(changed["output"]["id"], out["id"]);
    assert_eq!(changed["output"]["version"], 2);
    let revised = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        office_core::domain::metrics::metadata(&revised).unwrap()["supersedes"],
        "v1.md"
    );
    assert!(revised.contains("handoff: Next: review the draft."));
    assert!(revised.contains("decision: Decision: keep the current vendor."));
    assert!(path.with_file_name("v1.md").is_file());
    for text in [
        &revised,
        &std::fs::read_to_string(path.with_file_name("v1.md")).unwrap(),
    ] {
        assert!(
            !text.contains("credential-for-handoff-fixture"),
            "copied handoffs and version snapshots must be scrubbed"
        );
    }

    assert_eq!(
        f.call(f.input(), "office.capture")["output"]["deduplicated"],
        true
    );
}

#[test]
fn model_selections_are_digest_bound_cannot_rewrite_text_or_elevate_assistant_decisions() {
    let f = Fixture::new();
    let raw = [
        json!({"type":"user","sessionId":"fixture-session","timestamp":"2026-10-30T10:00:00Z","message":{"role":"user","content":"Always keep the exact wording.\nPerhaps we should move?\nI run the platform team."}}),
        json!({"type":"assistant","sessionId":"fixture-session","timestamp":"2026-10-30T10:01:00Z","message":{"role":"assistant","model":"claude-fixture","content":"Decision: move.\nAction: filed a draft."}}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    std::fs::write(f.transcript(), &raw).unwrap();
    let mut input = f.input();
    input["transcript_preview"] = json!(true);
    let preview = f.call(input.clone(), "office.capture");
    assert_eq!(preview["ok"], true, "{preview}");
    let preview = &preview["output"];
    assert_eq!(preview["lines"].as_array().unwrap().len(), 5);
    assert_eq!(preview["content_withheld"], false);
    let mut input = f.input();
    input["transcript_sha256"] = preview["transcript_sha256"].clone();
    input["selections"] = json!([{"line":0,"kind":"direction"},{"line":1,"kind":"decision"},{"line":2,"kind":"fact"},{"line":3,"kind":"decision"},{"line":4,"kind":"action"}]);
    let mut bad = input.clone();
    bad["transcript_sha256"] = json!("0".repeat(64));
    assert_eq!(
        f.call(bad, "office.capture")["error"]["code"],
        "invalid_session_transcript"
    );
    for selection in [
        json!({"line":0,"kind":"direction","text":"Fabricated"}),
        json!({"line":0,"kind":"handoff"}),
        json!({"line":100,"kind":"open"}),
    ] {
        let mut bad = input.clone();
        bad["selections"] = json!([selection]);
        assert_eq!(f.call(bad, "office.capture")["ok"], false);
    }
    let captured = f.call(input.clone(), "office.capture");
    assert_eq!(captured["ok"], true, "{captured}");
    let path = f
        .workspace()
        .join("library")
        .join(captured["output"]["path"].as_str().unwrap());
    let markdown = std::fs::read_to_string(&path).unwrap();
    assert!(markdown.contains("direction: Always keep the exact wording."));
    assert!(markdown.contains("open: Perhaps we should move?"));
    assert!(markdown.contains("fact: I run the platform team."));
    assert!(markdown.contains("open: Decision: move."));
    assert!(markdown.contains("action: Action: filed a draft."));
    // A second model answer cannot rewrite an already captured hash.
    input["selections"] = json!([]);
    assert_eq!(
        f.call(input, "office.capture")["output"]["deduplicated"],
        true
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), markdown);
    let id = &captured["output"]["id"];
    for field in ["orchestrator", "model", "session_id"] {
        let original = markdown
            .lines()
            .find(|line| line.starts_with(&format!("{field}:")))
            .unwrap();
        std::fs::write(&path, markdown.replace(original, &format!("{field}: true"))).unwrap();
        assert_eq!(
            f.call(json!({"id":id}), "office.source-show")["error"]["code"],
            "invalid_frontmatter"
        );
        std::fs::write(&path, markdown.replace(original, &format!("{field}: null"))).unwrap();
        assert_eq!(f.call(json!({"id":id}), "office.source-show")["ok"], true);
    }
}

#[test]
fn transcript_refusals_leave_library_history_unchanged_and_private_previews_withhold_content() {
    let f = Fixture::new();
    f.write("");
    let mut input = f.input();
    input["transcript_preview"] = json!(true);
    let preview = f.call(input, "office.capture");
    assert_eq!(preview["output"]["content_withheld"], true, "{preview}");
    assert_eq!(preview["output"]["lines"], json!([]));
    let head = || {
        let output = Command::new("git")
            .arg("-C")
            .arg(f.workspace().join("library"))
            .args(["rev-parse", "HEAD"])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", f.root.path().join("home"))
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    };
    let before = head();
    let mut wrong = f.input();
    wrong["session_id"] = json!("other-session");
    assert_eq!(
        f.call(wrong, "office.capture")["error"]["code"],
        "invalid_session_transcript"
    );
    let outside = f.root.path().join("outside.jsonl");
    std::fs::write(&outside, std::fs::read(f.transcript()).unwrap()).unwrap();
    let mut wrong = f.input();
    wrong["session_transcript_path"] = json!(outside);
    assert_eq!(
        f.call(wrong, "office.capture")["error"]["code"],
        "upload_outside_allowed_roots"
    );
    #[cfg(unix)]
    {
        let linked = f.workspace().join("link.jsonl");
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        let mut wrong = f.input();
        wrong["session_transcript_path"] = json!(linked);
        assert_eq!(
            f.call(wrong, "office.capture")["error"]["code"],
            "upload_outside_allowed_roots"
        );
    }
    for raw in [
        "{truncated",
        "{\"type\":\"session_meta\",\"payload\":{}}",
        "{\"type\":\"user\",\"sessionId\":\"fixture-session\",\"message\":{}}",
    ] {
        std::fs::write(f.transcript(), raw).unwrap();
        assert_eq!(
            f.call(f.input(), "office.capture")["error"]["code"],
            "invalid_session_transcript"
        );
    }
    assert_eq!(head(), before);
}
