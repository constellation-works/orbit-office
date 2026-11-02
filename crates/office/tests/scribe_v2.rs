//! Scribe v2 through the built binary (STD-04): model-free markers and the
//! trust rule in `office.scribe-pending`, the v2 pass of
//! `office.scribe-append` (one commit holding the SCR, its ledger lines and
//! any apply-mode page), and `office.scribe-query`/`office.scribe-ask` over
//! the new stores. Temp roots only; `HOME` and `TZ` are set on the child.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SESSION: &str = "0a31040c-e7e2-411a-8100-1eba1b8c1dde";
const OTHER: &str = "77ab12ff-0000-4000-8000-000000000000";
const CWD: &str = "/work/orbit-office";

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(fixture.workspace()).unwrap();
        assert_eq!(fixture.call("office.init", json!({}))["ok"], true);
        fixture
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("workspace")
    }

    fn library(&self) -> PathBuf {
        self.workspace().join("library")
    }

    fn call(&self, tool: &str, input: Value) -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", std::env::var("PATH").unwrap())
            .env("TZ", "UTC")
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

    fn refused(&self, tool: &str, input: Value) -> String {
        let response = self.call(tool, input);
        assert_eq!(response["ok"], false, "{response}");
        response["error"]["code"].as_str().unwrap().to_owned()
    }

    /// A transcript of `(role, text)` turns, one minute apart.
    fn transcript(&self, session: &str, hour: u32, turns: &[(&str, &str)]) -> PathBuf {
        let lines: Vec<String> = turns
            .iter()
            .enumerate()
            .map(|(n, (role, text))| {
                json!({"type":role,"sessionId":session,
                    "timestamp":format!("2026-11-01T{hour:02}:{n:02}:00Z"),
                    "cwd":CWD,"message":{"role":role,"content":text}})
                .to_string()
            })
            .collect();
        let path = self.workspace().join(format!("{session}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn pending(&self, session: &str, hour: u32, turns: &[(&str, &str)]) -> Value {
        let transcript = self.transcript(session, hour, turns);
        self.ok(
            "office.scribe-pending",
            json!({"session_id":session,"transcript":transcript,"provider":"claude-code",
                "cwd":CWD,"format":"v2"}),
        )
    }

    fn append(&self, session: &str, through: &str, findings: Value) -> Value {
        self.ok(
            "office.scribe-append",
            append_input(session, through, findings),
        )
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.library())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", self.root.path().join("home"))
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.library().join(relative)).unwrap()
    }
}

fn seed_ingested_orbit_office(fixture: &Fixture, count: usize) {
    let dir = fixture.library().join("sources/decisions");
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = String::from(
        "---\nid: decisions-orbit-office\nkind: decisions\ntitle: \"Decisions (project:orbit-office)\"\nscope: \"project:orbit-office\"\ncreated_at: \"2026-11-01T08:00:00Z\"\nsensitivity: normal\nmoved_sources: {}\n---\n",
    );
    for n in 1..=count {
        let entry = json!({
            "id": format!("dec-orbit-office-{n}"),
            "question": "Which slice ships first?", "question_by": "scribe",
            "options": [{"text": "ship v2 first", "ref": "orbit-decision:REQ-9#a1", "id": "a1", "consequence": "Ships v2."}],
            "chosen": "ship v2 first", "quote": "ship v2 first", "ref": "orbit-decision:REQ-9",
            "scope": "project:orbit-office",
            "context": {"task_type": null, "workspace": null, "task": "ORB-12345", "risk_tier": null},
            "decided_by": "human", "decided_at": "2026-11-01T09:05:00Z", "reverses": null,
            "orbit": {"request_id": "REQ-9", "gate": "ship", "category": "release", "topic": null, "answered_by": "human", "channel": "task"},
            "source": "orbit-decision:REQ-9"
        });
        text.push_str(&format!(
            "- dec-orbit-office-{n} 2026-11-01T09:05:00Z #decision {entry}\n"
        ));
    }
    std::fs::write(dir.join("orbit-office.md"), text).unwrap();
}

#[test]
fn append_refuses_ingestion_only_decision_fields_and_a_same_as_naming_no_entry() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &turns());
    let head = fixture.head();
    let out = fixture.append(
        SESSION,
        "m-0003",
        json!([
            {"msg":"m-0001","line":1,"tag":"decisions","decision": {
                "question":"Which slice ships first?","question_by":"scribe","options":"unrecoverable",
                "chosen":"ship v2 first","decided_by":"human","orbit":{"request_id":"REQ-1"}}},
            {"msg":"m-0001","line":1,"tag":"decisions","decision": {
                "question":"Which slice ships first?","question_by":"scribe","options":"unrecoverable",
                "chosen":"ship v2 first","decided_by":"human","confirms":"dec-orbit-office-1"}},
            {"msg":"m-0001","line":1,"tag":"decisions","decision": {
                "question":"Which slice ships first?","question_by":"scribe",
                "options":[{"text":"ship v2 first","msg":"m-0001","id":"a1"}],
                "chosen":"ship v2 first","decided_by":"human"}},
            {"msg":"m-0001","line":1,"tag":"decisions","decision": {
                "question":"Which slice ships first?","question_by":"scribe","options":"unrecoverable",
                "chosen":"ship v2 first","decided_by":"human","same_as":"dec-orbit-office-9"}},
        ]),
    );
    let codes: Vec<&str> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "invalid_decision",
            "invalid_decision",
            "invalid_decision",
            "decision_not_found"
        ],
        "{out}"
    );
    assert_eq!(out["written"], false, "{out}");
    assert_eq!(fixture.head(), head);
}

#[test]
fn append_sets_same_as_on_an_ingested_match_and_query_returns_the_new_fields() {
    let fixture = Fixture::new();
    seed_ingested_orbit_office(&fixture, 1);
    fixture.pending(SESSION, 9, &turns());
    let mut d = decision("ship v2 first", "human");
    d["context"] =
        json!({"task_type": null, "workspace": null, "task": "ORB-12345", "risk_tier": null});
    let out = fixture.append(
        SESSION,
        "m-0003",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":d}]),
    );
    assert_eq!(out["written"], true, "{out}");
    assert_eq!(out["refused"], json!([]), "{out}");
    let queried = fixture.ok(
        "office.scribe-query",
        json!({"mode":"decisions","scope":"project:orbit-office"}),
    );
    let entries = queried["decisions"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{queried}");
    assert_eq!(entries[0]["orbit"]["request_id"], "REQ-9", "{queried}");
    assert_eq!(entries[0]["options"][0]["id"], "a1", "{queried}");
    assert_eq!(entries[1]["same_as"], "dec-orbit-office-1", "{queried}");
    let shown = fixture.ok("office.source-show", json!({"id": "dec-orbit-office-2"}));
    assert_eq!(
        shown["decisions"][0]["same_as"], "dec-orbit-office-1",
        "{shown}"
    );
}

fn sections(summary: &str) -> Value {
    json!({"summary": summary, "goals": ["Build the scribe v2 slice for ORB-12345."],
        "outcome": ["The pass format is drafted."],
        "next": ["ORB-12345 — finish the query modes. owner: agent."]})
}

fn append_input(session: &str, through: &str, findings: Value) -> Value {
    json!({"session_id": session, "through": through,
        "sections": sections("Redesigned the scribe: markers, ledgers and the SCR handoff."),
        "findings": findings})
}

fn decision(chosen: &str, decided_by: &str) -> Value {
    json!({"question": "Which slice ships first?", "question_by": "scribe",
        "options": "unrecoverable", "chosen": chosen, "decided_by": decided_by})
}

const TYPED: &str = "#decision ship v2 first because: the feed is unusable\n\
we agreed #decision mid-sentence is prose\n\
```\n#decision inside a fence\n```\n\
> #decision inside a blockquote\n\
<pasted_content>\n#decision inside a paste\n</pasted_content>";

fn turns() -> Vec<(&'static str, &'static str)> {
    vec![
        ("user", TYPED),
        ("assistant", "#decision use sqlite for the index"),
        (
            "user",
            "#preferences @global always draft emails, never send",
        ),
    ]
}

#[test]
fn pending_returns_only_line_start_markers_in_typed_user_turns() {
    let fixture = Fixture::new();
    let pending = fixture.pending(SESSION, 9, &turns());
    assert_eq!(pending["format"], "v2");
    assert_eq!(pending["scope"], "project:orbit-office");
    let markers: Vec<(String, u64, Value)> = pending["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["msg"].as_str().unwrap().to_owned(),
                m["line"].as_u64().unwrap(),
                m["tag"].clone(),
            )
        })
        .collect();
    // The fenced, quoted, pasted and mid-sentence markers are not markers.
    assert_eq!(
        markers,
        vec![
            ("m-0001".into(), 1, json!("decisions")),
            ("m-0003".into(), 1, json!("preferences")),
        ],
        "{pending}"
    );
    let first = &pending["markers"][0];
    assert_eq!(first["by"], "user");
    assert_eq!(first["because"], "the feed is unusable");
    assert_eq!(first["scope"], "project:orbit-office");
    assert_eq!(pending["markers"][1]["scope"], "global");
    assert_eq!(pending["markers"][1]["scope_marked"], true);
    // The assistant's marker is recorded apart, by: agent.
    let agent = pending["agent_markers"].as_array().unwrap();
    assert_eq!(agent.len(), 1);
    assert_eq!(agent[0]["msg"], "m-0002");
    assert_eq!(agent[0]["by"], "agent");
    // The paste stays in the message text (ids are stable) but is marked.
    let message = &pending["messages"][0];
    assert!(message["text"].as_str().unwrap().contains("inside a paste"));
    assert!(!message["pasted"].as_array().unwrap().is_empty());
    assert_eq!(pending["scr"]["exists"], false);
    assert_eq!(pending["scr"]["id"], "SCR-20261101-0900-0a3104");
    assert_eq!(pending["previous"], Value::Null);
}

#[test]
fn append_refuses_unlisted_markers_inexact_quotes_and_wrong_authors() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &turns());
    let head = fixture.head();
    let out = fixture.append(
        SESSION,
        "m-0003",
        json!([
            // A mid-sentence "marker" was never returned.
            {"msg":"m-0001","line":2,"tag":"decisions","decision":decision("mid-sentence","human")},
            // A pasted marker was never returned either.
            {"msg":"m-0001","line":5,"tag":"decisions","decision":decision("paste","human")},
            // A quote that is not the cited message's text.
            {"msg":"m-0001","line":1,"tag":"decisions","quote":"#decision ship v3 first",
                "decision":decision("ship v2 first","human")},
            // A chosen span the message never said.
            {"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship v1 first","human")},
            // The user's marker claimed as an agent's decision.
            {"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship v2 first","agent")},
            // The agent's marker claimed as a human decision, and as a user's.
            {"msg":"m-0002","line":1,"tag":"decisions","decision":decision("use sqlite","human")},
            {"msg":"m-0002","line":1,"tag":"decisions","by":"user","decision":decision("use sqlite","agent")},
        ]),
    );
    let codes: Vec<&str> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "marker_not_found",
            "marker_not_found",
            "quote_mismatch",
            "quote_mismatch",
            "decided_by_mismatch",
            "decided_by_mismatch",
            "author_mismatch",
        ],
        "{out}"
    );
    // Refusals are reported, never written: no SCR, no ledger, no commit.
    assert_eq!(out["written"], false);
    assert_eq!(fixture.head(), head);
    assert!(
        !fixture
            .library()
            .join("sources/decisions/orbit-office.md")
            .exists()
    );
    assert!(
        !fixture
            .library()
            .join("sources/conversation/2026/11/SCR-20261101-0900-0a3104.md")
            .exists()
    );
}

#[test]
fn a_pass_commits_the_scr_and_its_ledger_lines_once_and_keeps_tags_append_only() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &turns());
    let before: usize = fixture
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    let out = fixture.append(
        SESSION,
        "m-0003",
        json!([
            {"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship v2 first","human")},
            {"msg":"m-0002","line":1,"tag":"decisions","decision":decision("use sqlite","agent")},
            {"msg":"m-0003","line":1,"tag":"preferences","matter":"email",
                "statement":"Always draft emails; never send them."},
        ]),
    );
    assert_eq!(out["written"], true, "{out}");
    assert_eq!(out["refused"], json!([]), "{out}");
    assert_eq!(out["id"], "SCR-20261101-0900-0a3104");
    let path = "sources/conversation/2026/11/SCR-20261101-0900-0a3104.md";
    // One commit holds the SCR and the ledger.
    let after: usize = fixture
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before + 1);
    let touched = fixture.git(&["show", "--name-only", "--format=", "HEAD"]);
    let mut touched: Vec<&str> = touched.lines().collect();
    touched.sort_unstable();
    assert_eq!(touched, [path, "sources/decisions/orbit-office.md"]);
    let scr = fixture.read(path);
    assert!(
        scr.contains("\nsession: \"0a31040c-e7e2-411a-8100-1eba1b8c1dde\"\n"),
        "{scr}"
    );
    assert!(scr.contains("\nprevious: null\n"), "{scr}");
    assert!(scr.contains("\ntitle_by: \"scribe\"\n"), "{scr}");
    assert!(
        scr.contains("## Summary\n(scribe) Redesigned the scribe"),
        "{scr}"
    );
    assert!(scr.contains(
        "### Decisions\n- T1 m-0001 2026-11-01T09:00+00:00 by: user scope: project:orbit-office \"#decision ship v2 first because: the feed is unusable\" ledger: dec-orbit-office-1\n- T2 m-0002 2026-11-01T09:01+00:00 by: agent scope: project:orbit-office \"#decision use sqlite for the index\" ledger: dec-orbit-office-2\n"
    ), "{scr}");
    assert!(scr.contains("### Preferences\n- T3 m-0003"), "{scr}");
    assert!(scr.contains("propose: propose_mode"), "{scr}");
    assert_eq!(out["needs_proposal"][0]["matter"], "email");
    // The ledger entries: the user's is human, the agent's is agent, with
    // the marker's stated reason and the SCR entry as their ref.
    let ledger = fixture.ok(
        "office.scribe-query",
        json!({"mode":"decisions","scope":"project:orbit-office"}),
    );
    let entries = ledger["decisions"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{ledger}");
    assert_eq!(entries[0]["id"], "dec-orbit-office-1");
    assert_eq!(entries[0]["decided_by"], "human");
    assert_eq!(entries[0]["ref"], "SCR-20261101-0900-0a3104#T1 m-0001");
    assert_eq!(entries[0]["because"]["text"], "the feed is unusable");
    assert_eq!(entries[1]["decided_by"], "agent");

    // A second pass rewrites the sections and keeps every Tags entry.
    let tags_before: Vec<String> = scr
        .lines()
        .filter(|l| l.starts_with("- T") && l.as_bytes()[3].is_ascii_digit())
        .map(str::to_owned)
        .collect();
    let mut longer = turns();
    longer.push(("assistant", "Noted."));
    longer.push(("user", "#decision cite tag entries by T ids"));
    let pending = fixture.pending(SESSION, 9, &longer);
    assert_eq!(pending["scribed_through"], "m-0003");
    assert_eq!(pending["scr"]["next_tag"], "T4");
    assert_eq!(pending["markers"].as_array().unwrap().len(), 1);
    let second = fixture.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0005",
            "sections":{"summary":"Settled the T-id anchors.","goals":[],"outcome":[],
                "next":["ORB-12346 — wire the query modes."]},
            "findings":[{"msg":"m-0005","line":1,"tag":"decisions",
                "decision":decision("cite tag entries by T ids","human")}]}),
    );
    assert_eq!(second["written"], true, "{second}");
    assert_eq!(second["added"][0]["id"], "T4");
    let rewritten = fixture.read(path);
    assert_eq!(tags_before.len(), 3);
    for line in &tags_before {
        assert!(rewritten.contains(&format!("{line}\n")), "{line} kept");
    }
    assert!(rewritten.contains("## Summary\n(scribe) Settled the T-id anchors.\n"));
    assert!(!rewritten.contains("Redesigned the scribe"));
    assert!(
        rewritten.contains("\ntitle: \"Settled the T-id anchors\"\n"),
        "{rewritten}"
    );
    assert!(rewritten.contains("- T4 m-0005"));
    assert!(rewritten.contains("ledger: dec-orbit-office-3"));
    // A retried pass with the same finding records nothing twice.
    let retry = fixture.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0005",
            "sections":{"summary":"Settled the T-id anchors.","goals":[],"outcome":[],
                "next":["ORB-12346 — wire the query modes."]},
            "findings":[{"msg":"m-0005","line":1,"tag":"decisions",
                "decision":decision("cite tag entries by T ids","human")}]}),
    );
    assert_eq!(retry["duplicates"][0]["entry"], "T4", "{retry}");

    // The next session in the scope names this SCR as its previous.
    fixture.pending(
        OTHER,
        11,
        &[("user", "#decision keep the v1 readers for the window")],
    );
    let next = fixture.append(
        OTHER,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions",
            "decision":decision("keep the v1 readers","human")}]),
    );
    assert_eq!(next["previous"], "SCR-20261101-0900-0a3104", "{next}");
    let next_scr = fixture.read("sources/conversation/2026/11/SCR-20261101-1100-77ab12.md");
    assert!(
        next_scr.contains("\nprevious: \"SCR-20261101-0900-0a3104\"\n"),
        "{next_scr}"
    );
}

#[test]
fn a_section_over_budget_or_a_next_item_without_a_task_id_is_refused() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &turns());
    let head = fixture.head();
    for (sections, code) in [
        (
            json!({"summary": "x".repeat(501), "goals": [], "outcome": [], "next": []}),
            "handoff_budget_exceeded",
        ),
        (
            json!({"summary": "ok", "goals": [], "outcome": [], "next": ["finish the query modes"]}),
            "handoff_task_id_incomplete",
        ),
    ] {
        assert_eq!(
            fixture.refused(
                "office.scribe-append",
                json!({"session_id":SESSION,"through":"m-0003","sections":sections,"findings":[]}),
            ),
            code
        );
    }
    assert_eq!(fixture.head(), head);
}

fn apply_preferences(fixture: &Fixture) {
    fixture.ok(
        "office.config-set",
        json!({"key":"scribe.tags.preferences","value":{"scope":"global",
            "sink":"preference_page","mode":"apply",
            "instruction":"A standing answer to a class of question, in the user's words."}}),
    );
}

#[test]
fn an_agent_preference_marker_never_changes_a_page_in_any_mode() {
    for apply in [false, true] {
        let fixture = Fixture::new();
        if apply {
            apply_preferences(&fixture);
        }
        fixture.pending(
            SESSION,
            9,
            &[
                ("user", "#preferences always draft emails, never send"),
                ("assistant", "#preferences tabs over spaces"),
            ],
        );
        let out = fixture.append(
            SESSION,
            "m-0002",
            json!([
                {"msg":"m-0001","line":1,"tag":"preferences","matter":"email",
                    "statement":"Always draft emails; never send them."},
                {"msg":"m-0002","line":1,"tag":"preferences","matter":"indentation",
                    "statement":"Use tabs."},
            ]),
        );
        assert_eq!(out["written"], true, "{out}");
        let page = fixture
            .library()
            .join("knowledge/domain/dom-preferences-global.md");
        let reasons: Vec<&str> = out["needs_proposal"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["reason"].as_str().unwrap())
            .collect();
        if apply {
            // The user's marker applies in the pass's one commit; the agent's
            // only proposes.
            assert_eq!(reasons, ["untrusted_candidate"], "{out}");
            assert_eq!(out["applied"][0]["page"], "dom-preferences-global");
            let text = std::fs::read_to_string(&page).unwrap();
            assert!(text.contains("## email\n"), "{text}");
            assert!(text.contains("SCR-20261101-0900-0a3104#T1"), "{text}");
            assert!(!text.contains("tabs") && !text.contains("indentation"));
            let touched = fixture.git(&["show", "--name-only", "--format=", "HEAD"]);
            assert!(touched.contains("knowledge/domain/dom-preferences-global.md"));
            assert!(touched.contains("SCR-20261101-0900-0a3104.md"));
        } else {
            assert_eq!(reasons, ["propose_mode", "untrusted_candidate"], "{out}");
            assert!(!page.exists());
        }
        assert!(
            out["needs_proposal"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["by"] == "agent" && p["matter"] == "indentation")
        );
    }
}

#[test]
fn a_paste_sized_or_conflicting_user_preference_only_proposes() {
    let fixture = Fixture::new();
    apply_preferences(&fixture);
    let long = format!("#preferences short replies\n{}", "filler\n".repeat(45));
    fixture.pending(SESSION, 9, &[("user", &long)]);
    let out = fixture.append(
        SESSION,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"preferences","matter":"replies",
            "statement":"Keep replies short."}]),
    );
    assert_eq!(
        out["needs_proposal"][0]["reason"], "paste_sized_turn",
        "{out}"
    );
    assert!(
        !fixture
            .library()
            .join("knowledge/domain/dom-preferences-global.md")
            .exists()
    );
}

#[test]
fn query_derives_reversed_by_and_current_and_returns_one_card_per_scope() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &[("user", "#decision ship v2 first")]);
    fixture.append(
        SESSION,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship v2 first","human")}]),
    );
    fixture.pending(
        OTHER,
        11,
        &[("user", "#decision ship v1 removal first after all")],
    );
    let mut reversing = decision("ship v1 removal first", "human");
    reversing["reverses"] = json!("dec-orbit-office-1");
    fixture.append(
        OTHER,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":reversing}]),
    );
    // A global session: its own card, beside the project's latest.
    let global = "99aa00bb-0000-4000-8000-000000000000";
    let transcript = fixture.transcript(
        global,
        12,
        &[("user", "#decision @global keep one card per scope")],
    );
    fixture.ok(
        "office.scribe-pending",
        json!({"session_id":global,"transcript":transcript,"provider":"claude-code",
            "cwd":CWD,"format":"v2","scope":"global"}),
    );
    fixture.append(
        global,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("keep one card per scope","human")}]),
    );

    let out = fixture.ok("office.scribe-query", json!({"mode":"decisions"}));
    let by_id = |id: &str| {
        out["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == id)
            .cloned()
            .unwrap()
    };
    assert_eq!(out["count"], 3, "{out}");
    assert_eq!(
        by_id("dec-orbit-office-1")["reversed_by"],
        "dec-orbit-office-2"
    );
    assert_eq!(by_id("dec-orbit-office-1")["current"], false);
    assert_eq!(by_id("dec-orbit-office-2")["current"], true);
    assert_eq!(
        by_id("dec-orbit-office-2")["reverses"],
        "dec-orbit-office-1"
    );
    assert_eq!(by_id("dec-global-1")["scope"], "global");
    let human = fixture.ok(
        "office.scribe-query",
        json!({"mode":"decisions","scope":"global","decided_by":"human","text":"one card"}),
    );
    assert_eq!(human["count"], 1, "{human}");

    let cards = fixture.ok("office.scribe-query", json!({"mode":"scr"}));
    let scopes: Vec<(&str, &str)> = cards["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["scope"].as_str().unwrap(), c["id"].as_str().unwrap()))
        .collect();
    assert_eq!(
        scopes,
        [
            ("global", "SCR-20261101-1200-99aa00"),
            ("project:orbit-office", "SCR-20261101-1100-77ab12"),
        ],
        "{cards}"
    );
    assert_eq!(cards["cards"][1]["previous"], "SCR-20261101-0900-0a3104");
    assert!(
        cards["cards"][1]["summary"]
            .as_str()
            .unwrap()
            .starts_with("(scribe) ")
    );
    let one = fixture.ok(
        "office.scribe-query",
        json!({"mode":"scr","id":"SCR-20261101-1100-77ab12"}),
    );
    assert_eq!(one["previous"]["id"], "SCR-20261101-0900-0a3104", "{one}");
    assert_eq!(
        one["scr"]["tags"][0]["citation"],
        "SCR-20261101-1100-77ab12#T1"
    );
}

#[test]
fn applied_preferences_are_queryable_by_scope_and_matter() {
    let fixture = Fixture::new();
    apply_preferences(&fixture);
    fixture.pending(
        SESSION,
        9,
        &[("user", "#preferences always draft emails, never send")],
    );
    fixture.append(
        SESSION,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"preferences","matter":"email",
            "statement":"Always draft emails; never send them."}]),
    );
    let out = fixture.ok(
        "office.scribe-query",
        json!({"mode":"preferences","scope":"global","matter":"email"}),
    );
    assert_eq!(out["count"], 1, "{out}");
    let section = &out["preferences"][0];
    assert_eq!(section["citation"], "dom-preferences-global#email");
    assert_eq!(
        section["statement"],
        "Always draft emails; never send them."
    );
    assert_eq!(section["stated_at"], "2026-11-01");
    assert_eq!(section["review_by"], "2027-01-30");
    assert_eq!(section["cites"], json!(["SCR-20261101-0900-0a3104#T1"]));
}

#[test]
fn a_private_session_writes_reference_only_ledger_entries_and_no_proposals() {
    let fixture = Fixture::new();
    fixture.ok(
        "office.config-set",
        json!({"key":"sensitivity.conversation","value":"private"}),
    );
    let pending = fixture.pending(
        SESSION,
        9,
        &[
            ("user", "#decision ship the secret plan"),
            ("user", "#preferences always draft, never send"),
        ],
    );
    assert_eq!(pending["content_withheld"], true);
    assert_eq!(pending["messages"], json!([]));
    assert_eq!(pending["markers"][0]["content_withheld"], true, "{pending}");
    assert!(pending["markers"][0].get("quote").is_none(), "{pending}");
    let out = fixture.append(
        SESSION,
        "m-0002",
        json!([
            {"msg":"m-0001","line":1,"tag":"decisions"},
            {"msg":"m-0002","line":1,"tag":"preferences","proposal":"prop-1"},
            {"msg":"m-0002","line":1,"tag":"preferences"},
        ]),
    );
    assert_eq!(out["written"], true, "{out}");
    assert!(!out.to_string().contains("secret"), "{out}");
    assert!(!pending.to_string().contains("secret"), "{pending}");
    assert_eq!(out["refused"][0]["code"], "preference_withheld");
    assert_eq!(out["needs_proposal"], json!([]));
    let ledger = fixture.read("sources/decisions/orbit-office.md");
    assert!(ledger.contains("\"private\":true"), "{ledger}");
    assert!(!ledger.contains("secret"), "{ledger}");
    let scr = fixture.read("sources/conversation/2026/11/SCR-20261101-0900-0a3104.md");
    assert!(scr.contains("\nsensitivity: \"private\"\n"), "{scr}");
    assert!(scr.contains("withheld: private"), "{scr}");
    // The v1 append never takes a private parse.
    assert_eq!(
        fixture.refused(
            "office.scribe-append",
            json!({"session_id":SESSION,"through":"m-0002","format":"v1","entries":[]}),
        ),
        "scribe_session_private"
    );
}

#[test]
fn v1_scr_files_stay_readable_beside_v2_ones() {
    let fixture = Fixture::new();
    // A v1 pass (no format) for one session, a v2 pass for another.
    let v1 = "11111111-0000-4000-8000-000000000000";
    let transcript = fixture.transcript(v1, 8, &[("user", "Decision: keep the v1 readers.")]);
    fixture.ok(
        "office.scribe-pending",
        json!({"session_id":v1,"transcript":transcript,"provider":"claude-code","cwd":CWD}),
    );
    let v1_out = fixture.ok(
        "office.scribe-append",
        json!({"session_id":v1,"through":"m-0001","title":"Keep the v1 readers","entries":[
            {"msg":"m-0001","tag":"decision","quote":"keep the v1 readers"}]}),
    );
    assert_eq!(v1_out["written"], true, "{v1_out}");
    fixture.pending(SESSION, 9, &[("user", "#decision ship v2 first")]);
    fixture.append(
        SESSION,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship v2 first","human")}]),
    );
    // The v1 query still reads the v1 line, and the v2 file adds none.
    let v1_query = fixture.ok("office.scribe-query", json!({"tag":"decision"}));
    assert_eq!(v1_query["count"], 1, "{v1_query}");
    assert_eq!(v1_query["results"][0]["quote"], "keep the v1 readers");
    // A v1 SCR never enters the v2 chain or cards.
    let cards = fixture.ok("office.scribe-query", json!({"mode":"scr"}));
    assert_eq!(cards["count"], 1, "{cards}");
    assert_eq!(cards["cards"][0]["previous"], Value::Null);
    // And a second v1 pass of the v1 session still works.
    let again = fixture.ok(
        "office.scribe-pending",
        json!({"session_id":v1,"transcript":transcript,"provider":"claude-code","cwd":CWD}),
    );
    assert_eq!(again["scr"]["exists"], true, "{again}");
}

#[cfg(unix)]
impl Fixture {
    /// `office.session-brief` through the binary, with Orbit stubbed to an
    /// empty task inventory (the callback channel office app.rs composes).
    fn brief(&self, input: Value) -> Value {
        use std::os::unix::fs::PermissionsExt;

        let stub = self.root.path().join("orbit-stub");
        if !stub.exists() {
            std::fs::write(
                &stub,
                "#!/usr/bin/env python3\nimport json, sys\nargs = sys.argv[1:]\nassert args[:2] == ['tool', 'run']\nrequest = json.loads(args[-1])\nif args[2] == 'orbit.task.list':\n    result = {'tasks': [], 'total': 0, 'truncated': False}\nelif args[2] == 'orbit.task.show':\n    result = {'id': request['id'], 'status': 'in-progress'}\nelse:\n    raise AssertionError(args)\nprint(json.dumps(result))\n",
            )
            .unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", std::env::var("PATH").unwrap())
            .env("TZ", "UTC")
            .env("ORBIT_BIN", &stub)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"schema_version":1,"tool":"office.session-brief","input":input,
            "context":{"workspace_root":self.workspace(),
                "plugin_state":self.root.path().join("state"),
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
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        response["output"].clone()
    }
}

/// Scribe v2 S6: through the built binary, the brief's handoff is the
/// project's latest SCR, the global SCR leads `global`, and a concurrent
/// SCR in the project appears in `other_sessions`.
#[cfg(unix)]
#[test]
fn session_brief_reads_the_latest_scr_per_project_and_global() {
    let fixture = Fixture::new();
    fixture.pending(SESSION, 9, &[("user", "#decision ship the brief")]);
    fixture.append(
        SESSION,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("ship the brief","human")}]),
    );
    fixture.pending(OTHER, 11, &[("user", "#decision keep going")]);
    fixture.append(
        OTHER,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("keep going","human")}]),
    );
    let global = "99aa00bb-0000-4000-8000-000000000000";
    let transcript = fixture.transcript(
        global,
        12,
        &[("user", "#decision @global keep one card per scope")],
    );
    fixture.ok(
        "office.scribe-pending",
        json!({"session_id":global,"transcript":transcript,"provider":"claude-code",
            "cwd":CWD,"format":"v2","scope":"global"}),
    );
    fixture.append(
        global,
        "m-0001",
        json!([{"msg":"m-0001","line":1,"tag":"decisions","decision":decision("keep one card per scope","human")}]),
    );

    let section = |brief: &Value, kind: &str| {
        brief["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["kind"] == kind)
            .cloned()
            .unwrap_or_else(|| panic!("no {kind} section in {brief}"))
    };
    for (full_brief, input) in [
        (
            true,
            json!({"project":"orbit-office","orchestrator":"claude"}),
        ),
        (
            false,
            json!({"view":"handoffs","project":"orbit-office","orchestrator":"claude"}),
        ),
    ] {
        let brief = fixture.brief(input);
        let handoff = &section(&brief, "handoff")["items"][0];
        assert_eq!(handoff["id"], "SCR-20261101-1100-77ab12", "{brief}");
        assert_eq!(
            handoff["link"],
            "sources/conversation/2026/11/SCR-20261101-1100-77ab12.md"
        );
        assert_eq!(handoff["entry_kind"], "handoff");
        let text = handoff["text"].as_str().unwrap();
        assert!(text.contains("(scribe) Redesigned the scribe"), "{text}");
        assert!(text.contains("Next:"), "{text}");
        assert!(text.contains("ORB-12345"), "{text}");
        let global_section = section(&brief, "global");
        assert_eq!(
            global_section["items"][0]["id"], "SCR-20261101-1200-99aa00",
            "{brief}"
        );
        if full_brief {
            let others = section(&brief, "other_sessions");
            let ids: Vec<&str> = others["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect();
            assert_eq!(ids, ["SCR-20261101-0900-0a3104"], "{brief}");
        }
    }
}
