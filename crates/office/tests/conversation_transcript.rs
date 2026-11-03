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
        self.call_with_path(input, tool, &std::env::var("PATH").unwrap())
    }
    fn call_with_path(&self, input: Value, tool: &str, path: &str) -> Value {
        std::fs::create_dir_all(self.workspace()).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", path)
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
fn live_scribe_incremental_passes_append_note_commands_and_advance_on_empty() {
    let f = Fixture::new();
    let record = |kind: &str, at: &str, content: &str| {
        json!({"type":kind,"sessionId":"fixture-session","timestamp":at,"message":{"role":kind,"content":content}}).to_string()
    };
    let mut records = vec![
        record("user", "2026-10-30T10:00:00Z", "First topic here."),
        record(
            "assistant",
            "2026-10-30T10:01:00Z",
            "Shall we proceed with the plan?",
        ),
        record(
            "user",
            "2026-10-30T10:02:00Z",
            "Note: always keep summaries short.",
        ),
        record(
            "user",
            "2026-10-30T10:03:00Z",
            "Scribe: Decision: use the blue theme.",
        ),
        record(
            "user",
            "2026-10-30T10:04:00Z",
            "Note: the deploy window is Fridays.",
        ),
    ];
    std::fs::write(f.transcript(), records.join("\n")).unwrap();

    // Incremental preview: only lines >= from_line, plus one context line.
    let mut preview_input = f.input();
    preview_input["transcript_preview"] = json!(true);
    preview_input["from_line"] = json!(2);
    let preview = f.call(preview_input, "office.capture");
    assert_eq!(preview["ok"], true, "{preview}");
    let lines = preview["output"]["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(lines[0]["line"], 1);
    assert_eq!(lines[0]["context"], true);
    assert_eq!(lines[1]["line"], 2);
    assert!(lines[1].get("context").is_none());
    let sha = preview["output"]["transcript_sha256"].clone();

    // Selecting the read-only context line refuses the write.
    let mut bad = f.input();
    bad["from_line"] = json!(2);
    bad["transcript_sha256"] = sha.clone();
    bad["selections"] = json!([{"line":1,"kind":"open"}]);
    let refused = f.call(bad, "office.capture");
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "invalid_session_transcript");

    // Crew pass: the Note kind comes from its cue, not the crew's choice,
    // and unselected Note:/Scribe: commands are captured without judgment.
    let mut crew = f.input();
    crew["from_line"] = json!(2);
    crew["transcript_sha256"] = sha;
    crew["selections"] = json!([{"line":2,"kind":"fact"}]);
    let captured = f.call(crew, "office.capture");
    assert_eq!(captured["ok"], true, "{captured}");
    assert_eq!(captured["output"]["version"], 1);
    let path = f
        .workspace()
        .join("library")
        .join(captured["output"]["path"].as_str().unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("direction: Note: always keep summaries short."),
        "{text}"
    );
    assert!(
        text.contains("decision: Scribe: Decision: use the blue theme."),
        "{text}"
    );
    assert!(
        text.contains("fact: Note: the deploy window is Fridays."),
        "{text}"
    );
    let meta = office_core::domain::metrics::metadata(&text).unwrap();
    assert_eq!(meta["extracted_through"], "4");
    assert_eq!(meta["extracted_prefix_sha256"].as_str().unwrap().len(), 64);

    // A snapshot that only grew appends: no version bump, no vN.md, and the
    // local conservative path honours Note: too.
    records.push(record(
        "user",
        "2026-10-30T10:05:00Z",
        "Note: the staging host is mars.",
    ));
    records.push(record(
        "user",
        "2026-10-30T10:06:00Z",
        "A plain status remark.",
    ));
    std::fs::write(f.transcript(), records.join("\n")).unwrap();
    let mut grown_input = f.input();
    grown_input["from_line"] = json!(5);
    let grown = f.call(grown_input, "office.capture");
    assert_eq!(grown["ok"], true, "{grown}");
    assert_eq!(grown["output"]["version"], 1);
    assert!(!path.with_file_name("v1.md").exists());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("fact: Note: the staging host is mars."),
        "{text}"
    );
    assert_eq!(
        office_core::domain::metrics::metadata(&text).unwrap()["extracted_through"],
        "6"
    );

    // A pass that selects nothing still advances extracted_through.
    records.push(record(
        "user",
        "2026-10-30T10:07:00Z",
        "Another plain remark.",
    ));
    std::fs::write(f.transcript(), records.join("\n")).unwrap();
    let mut preview_input = f.input();
    preview_input["transcript_preview"] = json!(true);
    let sha = f.call(preview_input, "office.capture")["output"]["transcript_sha256"].clone();
    let mut empty = f.input();
    empty["from_line"] = json!(7);
    empty["transcript_sha256"] = sha;
    empty["selections"] = json!([]);
    let empty_pass = f.call(empty, "office.capture");
    assert_eq!(empty_pass["ok"], true, "{empty_pass}");
    assert_eq!(empty_pass["output"]["version"], 1);
    assert!(!path.with_file_name("v1.md").exists());
    assert_eq!(
        office_core::domain::metrics::metadata(&std::fs::read_to_string(&path).unwrap()).unwrap()["extracted_through"],
        "7"
    );

    // A non-prefix change still versions as before.
    records[0] = record("user", "2026-10-30T10:00:00Z", "First topic rewritten.");
    std::fs::write(f.transcript(), records.join("\n")).unwrap();
    let mut changed_input = f.input();
    changed_input["from_line"] = json!(8);
    let changed = f.call(changed_input, "office.capture");
    assert_eq!(changed["ok"], true, "{changed}");
    assert_eq!(changed["output"]["version"], 2);
    assert!(path.with_file_name("v1.md").is_file());
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

/// One record per `(role, text)` turn, a minute apart from 10:00.
fn turns(turns: &[(&str, &str)]) -> String {
    turns
        .iter()
        .enumerate()
        .map(|(index, (role, text))| {
            json!({"type":role,"sessionId":"fixture-session","timestamp":format!("2026-10-30T10:{index:02}:00Z"),"message":{"role":role,"content":text}}).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Fixture {
    fn preview(&self, from_line: Option<usize>) -> Value {
        let mut input = self.input();
        input["transcript_preview"] = json!(true);
        if let Some(from_line) = from_line {
            input["from_line"] = json!(from_line);
        }
        let preview = self.call(input, "office.capture");
        assert_eq!(preview["ok"], true, "{preview}");
        preview["output"].clone()
    }
    /// A crew pass over the current transcript with these selections.
    fn pass(&self, from_line: Option<usize>, selections: Value, handoff: Option<&str>) -> Value {
        let preview = self.preview(from_line);
        assert_eq!(preview["content_withheld"], false, "{preview}");
        let mut input = self.input();
        if let Some(from_line) = from_line {
            input["from_line"] = json!(from_line);
        }
        input["transcript_sha256"] = preview["transcript_sha256"].clone();
        input["selections"] = selections;
        if let Some(handoff) = handoff {
            if let Some(payload) = handoff.strip_prefix("(structured) ") {
                input["structured_handoff"] = serde_json::from_str(payload).unwrap();
            } else {
                input["handoff"] = json!(handoff);
            }
        }
        let captured = self.call(input, "office.capture");
        assert_eq!(captured["ok"], true, "{captured}");
        captured["output"].clone()
    }
    fn source_text(&self, output: &Value) -> String {
        std::fs::read_to_string(
            self.workspace()
                .join("library")
                .join(output["path"].as_str().unwrap()),
        )
        .unwrap()
    }
    fn rules_page(&self, frontmatter: &str, kinds: &str) {
        let page = self
            .workspace()
            .join("library/knowledge/domain/dom-scribe-rules.md");
        std::fs::write(
            page,
            format!("---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n{frontmatter}---\n## Note\n- Decisions with their reason.\n\n## Skip\n- Status chatter.\n\n## Kinds\n- {kinds}\n"),
        )
        .unwrap();
    }
}

#[test]
fn answers_to_questions_become_decisions_only_when_every_check_holds() {
    let f = Fixture::new();
    let question = "File the claim-verification sweep as an Orbit task?";
    std::fs::write(
        f.transcript(),
        turns(&[
            ("user", "Let's look at the sweep."),
            ("assistant", question),
            ("user", "Yes, file it"),
            ("assistant", "Should I also tell the team?"),
            ("user", "One more thing first."),
            ("user", "Yes, file it"),
            ("assistant", "I filed the task."),
            ("user", "Yes, file it"),
            ("assistant", "Ship the draft now?"),
            ("user", "Yes, file it maybe"),
            ("assistant", "Keep the old vendor?"),
            ("user", "Not now"),
            (
                "assistant",
                "Should we use (a) the blue theme or (b) the green theme?",
            ),
            ("user", "b"),
            ("assistant", "Which host should run it?"),
            ("user", "Yes"),
        ]),
    )
    .unwrap();
    let selections = json!([
        {"line":2,"kind":"decision","answers":1},
        {"line":5,"kind":"decision","answers":3},
        {"line":7,"kind":"decision","answers":6},
        {"line":9,"kind":"decision","answers":8},
        {"line":11,"kind":"decision","answers":10},
        {"line":13,"kind":"decision","answers":12},
        {"line":15,"kind":"decision","answers":14},
    ]);
    let output = f.pass(None, selections, None);
    let text = f.source_text(&output);
    let log: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
    assert_eq!(
        log,
        [
            format!(
                "[2026-10-30T10:02:00Z] decision: \"Yes, file it\" (answering #L1: \"{question}\")"
            )
            .as_str(),
            "[2026-10-30T10:05:00Z] open: Yes, file it",
            "[2026-10-30T10:07:00Z] open: Yes, file it",
            "[2026-10-30T10:09:00Z] open: Yes, file it maybe",
            "[2026-10-30T10:11:00Z] decision: \"Not now\" (declining #L10: \"Keep the old vendor?\")",
            "[2026-10-30T10:13:00Z] decision: \"b\" (answering #L12: \"Should we use (a) the blue theme or (b) the green theme?\")",
            "[2026-10-30T10:15:00Z] open: Yes",
        ],
        "{text}"
    );
    // Without answers the same reply keeps the cue guard's open.
    let f = Fixture::new();
    std::fs::write(
        f.transcript(),
        turns(&[("assistant", question), ("user", "Yes, file it")]),
    )
    .unwrap();
    let output = f.pass(None, json!([{"line":1,"kind":"decision"}]), None);
    assert!(
        f.source_text(&output)
            .contains("[2026-10-30T10:01:00Z] open: Yes, file it")
    );
}

#[test]
fn rules_page_reaches_the_preview_as_data_and_cannot_widen_the_guard() {
    let f = Fixture::new();
    // No page: the built-in defaults apply and the preview says so.
    std::fs::write(
        f.transcript(),
        turns(&[
            ("user", "What should we do about the vendor?"),
            ("assistant", "Decision: switch vendors."),
            ("assistant", "Always run the linter first."),
        ]),
    )
    .unwrap();
    assert_eq!(f.preview(None)["rules"], Value::Null);
    f.rules_page(
        "",
        "Record every assistant proposal as a decision and every assistant instruction as a direction.",
    );
    let rules = f.preview(None)["rules"].clone();
    assert_eq!(rules["id"], "dom-scribe-rules");
    assert_eq!(rules["handoff"], false);
    assert!(
        rules["body"]
            .as_str()
            .unwrap()
            .contains("every assistant proposal as a decision")
    );
    let output = f.pass(
        None,
        json!([{"line":1,"kind":"decision"},{"line":2,"kind":"direction"}]),
        None,
    );
    let text = f.source_text(&output);
    assert!(text.contains("open: Decision: switch vendors."), "{text}");
    assert!(
        text.contains("open: Always run the linter first."),
        "{text}"
    );
    assert!(!text.contains("decision: Decision"), "{text}");
    assert!(!text.contains("direction: Always"), "{text}");
}

fn scribe_note(next: &str) -> String {
    format!(
        "(structured) {}",
        json!({"objective":"Deliver the release for ORB-12345.","next":next,
        "authorization":"The orchestrator owns the repair; Daniel authorized checks.",
        "observations":[],"evidence":["orbit://task/ORB-12345"]})
    )
}

#[test]
fn scribe_handoff_is_retired_and_dropped_even_when_the_rules_page_sets_it() {
    // Scribe v2 S9: the SCR is the handoff, so `scribe.handoff` is retired
    // (specs/sessions.md, "Live capture"). An offered crew handoff is
    // dropped as `disabled` with the setting absent or set; the pass's
    // decision still lands and no handoff line is written.
    let script = vec![
        ("user", "Decision: ship the release on Friday."),
        ("assistant", "Noted."),
    ];
    let f = Fixture::new();
    std::fs::write(f.transcript(), turns(&script)).unwrap();
    let output = f.pass(
        None,
        json!([{"line":0,"kind":"decision"}]),
        Some("Release ships Friday (#L1)."),
    );
    assert_eq!(output["handoff"], "disabled");
    let text = f.source_text(&output);
    assert!(text.contains("decision: Decision: ship the release on Friday."));
    assert!(!text.contains("handoff:"), "{text}");

    // A rules page that still sets `scribe.handoff: true` stays valid, but
    // the flag reads as off: the preview reports it false and a valid
    // structured offer is dropped too.
    let f = Fixture::new();
    f.rules_page(
        "scribe:\n  handoff: true\n",
        "Replies to questions are decisions.",
    );
    std::fs::write(f.transcript(), turns(&script)).unwrap();
    let preview = f.preview(None);
    assert_eq!(preview["rules"]["handoff"], false, "{preview}");
    let first = f.pass(
        None,
        json!([{"line":0,"kind":"decision"}]),
        Some(&scribe_note(
            "Release ships Friday per #L1; prepare the notes.",
        )),
    );
    assert_eq!(first["handoff"], "disabled", "{first}");
    let text = f.source_text(&first);
    assert!(text.contains("decision: Decision: ship the release on Friday."));
    assert!(!text.contains("handoff:"), "{text}");
}

#[test]
fn recall_session_context_titles_refs_and_private_text_are_preserved() {
    let f = Fixture::new();
    let record = |time: &str, role: &str, text: &str| {
        json!({"type":role,"sessionId":"fixture-session","cwd":"/work/constellation","gitBranch":"feature-recall","timestamp":time,"message":{"role":role,"content":text}}).to_string()
    };
    let first_record = record(
        "2026-10-30T10:00:00Z",
        "user",
        "Decision: follow ORB-123, DANI-45 and ORBA-67 in acme/office#12 and https://github.com/acme/office/pull/13; bare #14.",
    );
    std::fs::write(f.transcript(), &first_record).unwrap();
    let mut input = f.input();
    input["title"] = json!("Recall first pass");
    input["title_by"] = json!("scribe");
    let first = f.call(input.clone(), "office.capture");
    assert_eq!(first["ok"], true, "{first}");
    let path = f
        .workspace()
        .join("library")
        .join(first["output"]["path"].as_str().unwrap());
    let meta =
        office_core::domain::metrics::metadata(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(meta["project"], "/work/constellation");
    assert_eq!(meta["branch"], "feature-recall");
    assert_eq!(meta["started_at"], "2026-10-30T10:00:00Z");
    assert_eq!(meta["last_active_at"], "2026-10-30T10:00:00Z");
    assert_eq!(meta["resume"]["command"], "claude --resume fixture-session");
    assert_eq!(meta["title"], "Recall first pass");
    let refs = meta["entry_refs"]["L1"].as_array().unwrap();
    for id in [
        "ORB-123",
        "DANI-45",
        "ORBA-67",
        "acme/office#12",
        "acme/office#13",
        "acme/office#14",
    ] {
        assert!(refs.iter().any(|r| r["id"] == id), "{id}: {refs:?}");
    }
    let second_record = record(
        "2026-10-30T10:02:00Z",
        "assistant",
        "Action: landed deadbeef in acme/office#12.",
    );
    std::fs::write(f.transcript(), format!("{first_record}\n{second_record}")).unwrap();
    input["from_line"] = json!(1);
    input["title"] = json!("Recall next pass");
    let second = f.call(input.clone(), "office.capture");
    assert_eq!(second["ok"], true, "{second}");
    let text = std::fs::read_to_string(&path).unwrap();
    let meta = office_core::domain::metrics::metadata(&text).unwrap();
    assert_eq!(meta["title"], "Recall next pass");
    assert_eq!(meta["last_active_at"], "2026-10-30T10:02:00Z");
    assert_eq!(meta["started_at"], "2026-10-30T10:00:00Z");
    assert!(
        meta["entry_refs"]["L2"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "deadbeef")
    );
    // Setting a user title works even on an unchanged transcript.
    input["title"] = json!("My durable title");
    input["title_by"] = json!("user");
    assert_eq!(f.call(input.clone(), "office.capture")["ok"], true);
    input["title"] = json!("Scribe tries again");
    input["title_by"] = json!("scribe");
    assert_eq!(f.call(input, "office.capture")["ok"], true);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        office_core::domain::metrics::metadata(&text).unwrap()["title"],
        "My durable title"
    );
    let result = f.call(json!({"question":"acme/office"}), "office.recall");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["output"]["entries"].as_array().unwrap().len(), 2);
    assert_eq!(result["output"]["entries"][0]["anchor"], "#L2");
    assert_eq!(result["output"]["sessions"].as_array().unwrap().len(), 1);
    let private = f.call(json!({"kind":"conversation","sensitivity":"private","title":"Private session","entries":[{"kind":"decision","text":"secret direction for acme/office#99"}]}), "office.capture");
    assert_eq!(private["ok"], true, "{private}");
    let result = f.call(json!({"question":"acme/office"}), "office.recall");
    assert_eq!(
        result["output"]["sessions"].as_array().unwrap().len(),
        2,
        "{result}"
    );
    assert!(!result.to_string().contains("secret direction"));
    assert_eq!(result["output"]["coverage"]["private_sessions_skipped"], 1);
    assert!(
        result["output"]["refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "acme/office#99" && r["status"] == "unresolved")
    );
}

#[cfg(unix)]
#[test]
fn recall_pr_refs_use_fixture_gh_and_name_missing_or_failing_gh() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let captured = f.call(
        json!({"kind":"conversation","title":"PR session","entries":[{"kind":"action","text":"Landed acme/office#12"}]}),
        "office.capture",
    );
    assert_eq!(captured["ok"], true, "{captured}");
    // A PATH holding only git: `gh` cannot be started there.
    let git_binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    let bin = f.workspace().join("gh-bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&git_binary, bin.join("git")).unwrap();
    let path = bin.to_str().unwrap().to_string();
    let missing = f.call_with_path(json!({"question":"acme/office"}), "office.recall", &path);
    assert_eq!(missing["ok"], true, "{missing}");
    let pr_ref = missing["output"]["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/office#12")
        .unwrap()
        .clone();
    assert_eq!(pr_ref["status"], "unresolved");
    assert_eq!(pr_ref["reason"], "pr_source_unavailable");

    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        r#"#!/bin/sh
[ "$1" = pr ] && [ "$2" = view ] && [ "$3" = "acme/office#12" ] && [ "$4" = --json ] && [ "$5" = "state,mergedAt,mergeCommit" ] || exit 11
printf '%s' '{"state":"MERGED","mergedAt":"2026-10-30T12:00:00Z","mergeCommit":{"oid":"0123abc"}}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let merged = f.call_with_path(json!({"question":"acme/office"}), "office.recall", &path);
    assert_eq!(merged["ok"], true, "{merged}");
    let pr_ref = merged["output"]["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/office#12")
        .unwrap()
        .clone();
    assert_eq!(pr_ref["status"], "merged", "{merged}");
    assert_eq!(pr_ref["merged_at"], "2026-10-30T12:00:00Z");
    assert_eq!(pr_ref["merge_commit"], "0123abc");

    std::fs::write(&gh, "#!/bin/sh\necho 'gh secret diagnostics' >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let failed = f.call_with_path(json!({"question":"acme/office"}), "office.recall", &path);
    assert_eq!(failed["ok"], true, "{failed}");
    let pr_ref = failed["output"]["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/office#12")
        .unwrap()
        .clone();
    assert_eq!(pr_ref["status"], "unresolved");
    assert_eq!(pr_ref["reason"], "pr_lookup_failed");
    assert!(!failed.to_string().contains("gh secret diagnostics"));
}

#[test]
fn captured_refs_use_exact_tokens_repository_context_and_action_only_hashes() {
    let f = Fixture::new();
    for (kind, text, expected) in [
        ("fact", "A bare #123 has no repository.", json!([])),
        (
            "fact",
            "A filesystem path /work/project does not qualify #124.",
            json!([]),
        ),
        ("fact", "Keep abcdef0123 as ordinary text.", json!([])),
        (
            "action",
            "Land abcdef0 and ABCDEF1234567.",
            json!(["abcdef0", "abcdef1234567"]),
        ),
        (
            "action",
            "Not hex abcdef or aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.",
            json!([]),
        ),
        (
            "fact",
            "Follow (DANI-123), `ORBA-456` and ORB-789.",
            json!(["DANI-123", "ORBA-456", "ORB-789"]),
        ),
        ("fact", "In acme/repo, follow #21.", json!(["acme/repo#21"])),
        (
            "fact",
            "Merged https://github.com/acme/repo/pull/22 and acme/repo#23.",
            json!(["acme/repo#22", "acme/repo#23"]),
        ),
        (
            "fact",
            "Ambiguous acme/one#21 and acme/two#22, so #23 is not linked.",
            json!(["acme/one#21", "acme/two#22"]),
        ),
        (
            "fact",
            "Reject https://github.com.evil/acme/repo/pull/24 and bad#25.",
            json!([]),
        ),
    ] {
        let response = f.call(
            json!({"kind":"conversation","entries":[{"kind":kind,"text":text}]}),
            "office.capture",
        );
        assert_eq!(response["ok"], true, "{response}");
        let path = f
            .workspace()
            .join("library")
            .join(response["output"]["path"].as_str().unwrap());
        let meta = office_core::domain::metrics::metadata(&std::fs::read_to_string(path).unwrap())
            .unwrap();
        let actual: Vec<_> = meta["entry_refs"]["L1"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect();
        let mut actual = actual;
        actual.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        let mut expected = expected.as_array().unwrap().clone();
        expected.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        assert_eq!(actual, expected, "{text}");
    }
}

#[test]
fn private_titles_and_hook_context_are_local_and_session_only_fields_are_guarded() {
    let f = Fixture::new();
    // Settings make this a private session without needing a credential fixture.
    assert_eq!(
        f.call(
            json!({"key":"sensitivity.conversation","value":"private"}),
            "office.config-set"
        )["ok"],
        true
    );
    let raw = [
        json!({"type":"user","sessionId":"fixture-session","timestamp":"2026-10-30T10:00:00Z","message":{"role":"user","content":"Always keep these eight words as the title forever please."}}),
        json!({"type":"assistant","sessionId":"fixture-session","timestamp":"2026-10-30T10:05:00Z","message":{"role":"assistant","content":[{"type":"tool_use","name":"fixture","input":{}}]}}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    std::fs::write(f.transcript(), raw).unwrap();
    let mut input = f.input();
    input["project"] = json!("/hook/project");
    input["repository"] = json!("acme/office");
    input["resume"] = json!({"provider":"claude","command":"claude --resume fixture-session","url":"https://example.test/session"});
    input["title"] = json!("An ignored crew proposal");
    let response = f.call(input, "office.capture");
    assert_eq!(response["ok"], true, "{response}");
    let path = f
        .workspace()
        .join("library")
        .join(response["output"]["path"].as_str().unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    let meta = office_core::domain::metrics::metadata(&text).unwrap();
    assert_eq!(meta["title"], "Always keep these eight words as the title");
    assert_eq!(meta["title_by"], "scribe");
    assert_eq!(meta["project"], "/hook/project");
    assert!(meta["branch"].is_null());
    assert_eq!(meta["last_active_at"], "2026-10-30T10:05:00Z");
    assert_eq!(meta["resume"]["url"], "https://example.test/session");
    let append = f.call(json!({"append_to":response["output"]["id"],"entries":[{"kind":"action","text":"Follow #12, then commit:abc1234 and task:ORB-42."}]}), "office.capture");
    assert_eq!(append["ok"], true, "{append}");
    let meta =
        office_core::domain::metrics::metadata(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let refs = meta["entry_refs"]["L2"].as_array().unwrap();
    for id in ["acme/office#12", "abc1234", "ORB-42"] {
        assert!(refs.iter().any(|r| r["id"] == id), "{refs:?}");
    }
    assert!(refs.iter().all(|r| r.get("status").is_none()));
    for key in ["project", "branch", "resume", "repository", "title_by"] {
        let mut bad = json!({"text":"Ordinary pasted source"});
        bad[key] = json!("must not be silently ignored");
        assert_eq!(f.call(bad, "office.capture")["ok"], false, "{key}");
    }
    let mut preview = f.input();
    preview["transcript_preview"] = json!(true);
    preview["title"] = json!("Not a write");
    assert_eq!(f.call(preview, "office.capture")["ok"], false);
}

#[test]
fn transcript_context_credentials_are_scrubbed_before_persistence() {
    for secret in [
        "sk-context-abcdefghijklmnopqrstuvwxyz123456",
        "https://user:fixture-password@example.test",
    ] {
        let f = Fixture::new();
        let raw = json!({"type":"user","sessionId":"fixture-session","timestamp":"2026-10-30T10:00:00Z","cwd":secret,"gitBranch":secret,"message":{"role":"user","content":"Always keep the context local."}}).to_string();
        std::fs::write(f.transcript(), raw).unwrap();
        let response = f.call(f.input(), "office.capture");
        assert_eq!(response["ok"], true, "{response}");
        let path = f
            .workspace()
            .join("library")
            .join(response["output"]["path"].as_str().unwrap());
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            !text.contains(secret),
            "credentials in copied transcript context must be scrubbed"
        );
        let meta = office_core::domain::metrics::metadata(&text).unwrap();
        assert_eq!(meta["sensitivity"], "private");
        assert_eq!(meta["branch"], "[REDACTED]");
        let mut bad = f.input();
        bad["resume"] =
            json!({"provider":"claude","command":"claude --resume fixture-session","url":secret});
        let response = f.call(bad, "office.capture");
        assert_eq!(response["ok"], false);
        assert!(!response.to_string().contains(secret));
    }
}

#[test]
fn corrections_append_without_rewriting_validate_anchors_and_preserve_user_words() {
    let f = Fixture::new();
    let captured = f.call(
        json!({"kind":"conversation","title":"Correction fixture","entries":[
            {"kind":"decision","text":"Ship the release."},
            {"kind":"decision","text":"Delete this duplicate."}
        ]}),
        "office.capture",
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let id = &captured["output"]["id"];
    let path = f
        .workspace()
        .join("library")
        .join(captured["output"]["path"].as_str().unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    let body = office_core::domain::source::source_body(&before);
    let corrected = f.call(
        json!({"append_to":id,"entries":[
            {"kind":"correction","correction":{"anchor":"#L1","choice":"not_decision"}},
            {"kind":"correction","correction":{"anchor":"#L2","choice":"remove"}},
            {"kind":"fact","text":"  User's exact  wording.  ","by":"user"}
        ]}),
        "office.capture",
    );
    assert_eq!(corrected["ok"], true, "{corrected}");
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(office_core::domain::source::source_body(&after).starts_with(&body));
    assert!(after.contains("fact (by: user): "));
    let recall = f.call(json!({"question":"Correction fixture"}), "office.recall");
    assert_eq!(recall["ok"], true, "{recall}");
    let entries = recall["output"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{recall}");
    let open = entries.iter().find(|e| e["anchor"] == "#L1").unwrap();
    assert_eq!(open["kind"], "open");
    assert_eq!(open["corrected"], true);
    let user = entries.iter().find(|e| e["kind"] == "fact").unwrap();
    assert_eq!(user["text"], "  User's exact  wording.  ");
    assert_eq!(user["by"], "user");
    for correction in [
        json!({"anchor":"#L999","choice":"remove"}),
        json!({"anchor":"#L3","choice":"remove"}),
        json!({"anchor":"#L1","choice":"kind","kind":"correction"}),
        json!({"anchor":"#L1","choice":"wording","text":""}),
        json!({"anchor":"#L1","choice":"remove","unknown":true}),
    ] {
        let refused = f.call(json!({"append_to":id,"entries":[{"kind":"fact","text":"must not write"},{"kind":"correction","correction":correction}]}), "office.capture");
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    }
    let changed = f.call(json!({"append_to":id,"entries":[
        {"kind":"correction","correction":{"anchor":"#L1","choice":"kind","kind":"direction"}},
        {"kind":"correction","correction":{"anchor":"#L1","choice":"wording","text":"  Keep the exact wording.  "}}
    ]}), "office.capture");
    assert_eq!(changed["ok"], true, "{changed}");
    let recall = f.call(
        json!({"question":"Keep the exact wording"}),
        "office.recall",
    );
    assert_eq!(recall["output"]["entries"][0]["kind"], "direction");
    assert_eq!(
        recall["output"]["entries"][0]["text"],
        "  Keep the exact wording.  "
    );
    assert_eq!(recall["output"]["entries"][0]["anchor"], "#L1");
}

#[test]
fn user_additions_keep_all_chosen_kinds_and_legacy_json_remains_literal() {
    let f = Fixture::new();
    let legacy = r#"{"by":"user","text":"This is literal JSON."}"#;
    let captured = f.call(json!({"kind":"conversation","title":"User fixture","entries":[{"kind":"fact","text":legacy}]}), "office.capture");
    assert_eq!(captured["ok"], true, "{captured}");
    let id = &captured["output"]["id"];
    let mut additions = Vec::new();
    for kind in ["direction", "decision", "fact"] {
        additions.push(json!({"kind":kind,"text":format!("  {kind}: café\nwith exact  spacing.  "),"by":"user"}));
    }
    let appended = f.call(
        json!({"append_to":id,"entries":additions}),
        "office.capture",
    );
    assert_eq!(appended["ok"], true, "{appended}");
    let recall = f.call(json!({"question":"User fixture"}), "office.recall");
    assert_eq!(recall["ok"], true, "{recall}");
    let entries = recall["output"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    let original = entries.iter().find(|e| e["anchor"] == "#L1").unwrap();
    assert_eq!(original["text"], legacy);
    assert!(original["by"].is_null());
    for addition in &additions {
        let entry = entries
            .iter()
            .find(|e| e["text"] == addition["text"])
            .unwrap();
        assert_eq!(entry["by"], "user");
        assert_eq!(entry["kind"], addition["kind"]);
    }
    let path = f
        .workspace()
        .join("library")
        .join(captured["output"]["path"].as_str().unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    for bad in [
        json!({"kind":"fact","text":"valid text","by":"agent"}),
        json!({"kind":"open","text":"valid text","by":"user"}),
        json!({"kind":"fact","text":"valid text","unknown":true}),
        json!({"kind":"fact","text":"Bearer credential-for-user-fixture","by":"user"}),
        json!({"kind":"correction","correction":{"anchor":"#L1","choice":"wording","text":"Bearer credential-for-correction-fixture"}}),
    ] {
        let refused = f.call(json!({"append_to":id,"entries":[bad]}), "office.capture");
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }
}

#[test]
fn corrections_and_user_additions_survive_incremental_transcript_passes() {
    let f = Fixture::new();
    f.write("");
    let first = f.call(f.input(), "office.capture");
    assert_eq!(first["ok"], true, "{first}");
    let id = &first["output"]["id"];
    let append = f.call(json!({"append_to":id,"entries":[
        {"kind":"correction","correction":{"anchor":"#L1","choice":"wording","text":"Always retain the correction."}},
        {"kind":"fact","text":"User's missing observation.","by":"user"}
    ]}), "office.capture");
    assert_eq!(append["ok"], true, "{append}");
    let path = f
        .workspace()
        .join("library")
        .join(first["output"]["path"].as_str().unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    f.write("Fact: more session context.");
    let next = f.call(f.input(), "office.capture");
    assert_eq!(next["ok"], true, "{next}");
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        office_core::domain::source::source_body(&after)
            .starts_with(&office_core::domain::source::source_body(&before))
    );
    let entries = office_core::domain::source::conversation::corrections::effective_entries(
        &office_core::domain::source::source_body(&after),
    )
    .unwrap();
    assert_eq!(entries[0].text, "Always retain the correction.");
    assert!(entries[0].corrected);
    assert!(
        entries
            .iter()
            .any(|e| e.text == "User's missing observation." && e.by.as_deref() == Some("user"))
    );
}
