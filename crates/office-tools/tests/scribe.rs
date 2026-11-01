//! `office.scribe-pending` and `office.scribe-append` through public exec
//! dispatch, with an in-memory Library and a fixed clock in a fixed zone
//! (specs/scribe.md).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::time::{Duration, SystemTime};

use office_core::ports::{Clock, Library, Ports};
use office_testkit::{envelope, fakes::FakeLibrary};
use serde_json::{Value, json};

const SESSION: &str = "0a31040c-e7e2-411a-8100-1eba1b8c1dde";

/// 2026-11-01T05:00:00Z, which is 22:00 on 31 October in Los Angeles.
struct ZonedClock;
impl Clock for ZonedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_509_200)
    }
    fn time_zone(&self) -> Option<String> {
        Some("America/Los_Angeles".into())
    }
}

fn record(role: &str, minute: u32, content: &Value) -> String {
    let mut message = json!({"role": role, "content": content});
    if role == "assistant" {
        message["model"] = json!("claude-opus-5-5");
    }
    json!({"type": role, "sessionId": SESSION, "timestamp": format!("2026-11-01T03:{minute:02}:00Z"),
        "cwd": "/Users/daniel/workspace/constellation", "gitBranch": "agent-main", "message": message})
    .to_string()
}

/// `count` conversational records, alternating user and assistant, plus a
/// tool result and a sidechain the adapter drops.
fn transcript(count: u32) -> String {
    let mut lines = vec![json!({"type":"summary","summary":"ignored"}).to_string()];
    for n in 0..count {
        let line = match n {
            0 => record(
                "user",
                7,
                &json!(
                    "Scribe v1 design: lets go one step at a time.\nKeep the SCR file grep-able."
                ),
            ),
            1 => record(
                "assistant",
                8,
                &json!([{"type":"text","text":"Merged as 7266911, PR#268 is in."},{"type":"tool_use","name":"Bash","input":{"command":"gh pr merge"}}]),
            ),
            2 => record(
                "user",
                9,
                &json!([{"type":"tool_result","content":"secret output"}]),
            ),
            n if n % 2 == 1 => record("assistant", 9 + n, &json!(format!("Assistant reply {n}."))),
            n => record("user", 9 + n, &json!(format!("User message {n}."))),
        };
        lines.push(line);
    }
    lines.push(
        json!({"type":"user","isSidechain":true,"sessionId":SESSION,"timestamp":"2026-11-01T04:59:00Z","message":{"role":"user","content":"sidechain"}})
            .to_string(),
    );
    lines.join("\n") + "\n"
}

fn ports(mut library: FakeLibrary) -> Ports<'static> {
    library
        .session_transcripts
        .insert("short.jsonl".into(), transcript(6));
    library
        .session_transcripts
        .insert("long.jsonl".into(), transcript(17));
    library.host = Some("macbook".into());
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.clock = Box::new(ZonedClock);
    ports
}

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn output(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let response = call(ports, tool, input);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

fn pending(ports: &Ports<'_>, transcript: &str) -> Value {
    output(
        ports,
        "office.scribe-pending",
        json!({"session_id":SESSION,"transcript":transcript,"provider":"claude-code","cwd":"/Users/daniel/workspace/constellation"}),
    )
}

fn append(ports: &Ports<'_>, input: Value) -> Value {
    let mut input = input;
    input["session_id"] = json!(SESSION);
    output(ports, "office.scribe-append", input)
}

fn scr(ports: &Ports<'_>) -> (String, String) {
    let files = ports.library.scribe_files().unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    (files[0].path.clone(), files[0].markdown.clone().unwrap())
}

fn ids(messages: &Value) -> Vec<(String, bool)> {
    messages
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["id"].as_str().unwrap().to_string(), m["context"] == true))
        .collect()
}

#[test]
fn pending_writes_stable_ids_and_returns_new_messages_with_context_and_default_rules() {
    let ports = ports(FakeLibrary::initialized());
    let first = pending(&ports, "short.jsonl");
    assert_eq!(
        first["parsed"],
        format!(".orbit/tmp/scribe/{SESSION}.jsonl")
    );
    assert_eq!(first["scribed_through"], Value::Null);
    // The tool result record keeps no text, so six records give five messages.
    assert_eq!(first["message_count"], 5);
    let messages = first["messages"].as_array().unwrap();
    assert_eq!(
        messages[0],
        json!({"id":"m-0001","role":"user","at":"2026-11-01T03:07:00Z","text":"Scribe v1 design: lets go one step at a time.\nKeep the SCR file grep-able."})
    );
    assert_eq!(
        messages[1]["text"], "Merged as 7266911, PR#268 is in.",
        "tool use is not text"
    );
    assert!(messages.iter().all(|m| m["context"].is_null()));
    // What the new file will be called: local start, slug from Daniel's first message.
    assert_eq!(first["scr"]["exists"], false);
    assert_eq!(
        first["scr"]["path"],
        "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time.md"
    );
    // No rules page: the spec defaults, topics fall back to the project name.
    assert_eq!(first["rules"]["source"], "defaults");
    assert_eq!(first["rules"]["topics"], json!(["constellation"]));
    let tags: Vec<&str> = first["rules"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tags,
        [
            "decision",
            "direction",
            "question",
            "todo",
            "reminder",
            "fact",
            "landed",
            "preference",
            "why",
            "agent-decision",
            "pending",
            "resolved",
            "incident"
        ]
    );
    assert_eq!(
        first["rules"]["matters"],
        json!(["release-with-open-defects"])
    );
    assert_eq!(
        first["rules"]["models"],
        json!({"ask": "haiku@low", "scribe": "haiku@medium"})
    );

    let short = ports.library.read_scribe_parse(SESSION).unwrap().unwrap();
    let lines: Vec<Value> = short
        .messages
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[4]["id"], "m-0005");
    let meta: Value = serde_json::from_str(&short.meta).unwrap();
    assert_eq!(meta["host"], "macbook");
    assert_eq!(meta["model"], "claude-opus-5-5");

    // A grown transcript keeps every earlier line and only adds ids.
    let grown = pending(&ports, "long.jsonl");
    let long = ports.library.read_scribe_parse(SESSION).unwrap().unwrap();
    assert!(long.messages.starts_with(&short.messages));
    assert_eq!(grown["message_count"], 16);

    // After scribing through m-0013, only m-0014.. are new, with ten context messages.
    append(&ports, json!({"through":"m-0013","entries":[]}));
    let after = pending(&ports, "long.jsonl");
    assert_eq!(after["scribed_through"], "m-0013");
    assert_eq!(after["scr"]["exists"], true);
    let expected: Vec<(String, bool)> = (4..=16).map(|n| (format!("m-{n:04}"), n <= 13)).collect();
    assert_eq!(ids(&after["messages"]), expected);
    assert_eq!(after["new_count"], 3);
}

#[test]
fn pending_reads_rule_sections_from_the_rules_page() {
    let library = FakeLibrary::initialized();
    library
        .write_document(
            Path::new("knowledge/domain/dom-scribe-rules.md"),
            "---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n---\n## Tags\n- `decision`: Daniel settles a choice.\n  - lets go one step at a time\n- `landed`: A PR merged.\n\n## Topics\n- scribe: the scribe feature\n- recall\n\n## Matters\n- release-with-open-defects\n- other-matter\n\n## Capture\n- Skip status chatter.\n\n## Models\n- query: none\n- ask: luna@low\n- scribe: luna@medium\n",
        )
        .unwrap();
    let ports = ports(library);
    let rules = pending(&ports, "short.jsonl")["rules"].clone();
    assert_eq!(rules["source"], "page");
    assert_eq!(rules["topics"], json!(["scribe", "recall"]));
    assert_eq!(
        rules["matters"],
        json!(["release-with-open-defects", "other-matter"])
    );
    assert_eq!(rules["capture"], json!(["Skip status chatter."]));
    assert_eq!(
        rules["tags"],
        json!([
            {"name":"decision","meaning":"Daniel settles a choice.","examples":["lets go one step at a time"],"authors":"user"},
            {"name":"landed","meaning":"A PR merged.","examples":[],"authors":"any"}
        ])
    );
    assert_eq!(
        rules["models"],
        json!({"ask": "luna@low", "scribe": "luna@medium"})
    );
}

#[test]
fn pending_returns_default_models_without_models_section_and_defaults_for_old_sections() {
    // A v1 page without Models falls back to the spec defaults.
    let library = FakeLibrary::initialized();
    library
        .write_document(
            Path::new("knowledge/domain/dom-scribe-rules.md"),
            "---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n---\n## Tags\n- `decision`: Daniel settles a choice.\n\n## Topics\n- scribe\n\n## Capture\n- Skip status chatter.\n",
        )
        .unwrap();
    let v1_ports = ports(library);
    let rules = pending(&v1_ports, "short.jsonl")["rules"].clone();
    assert_eq!(rules["source"], "page");
    assert_eq!(
        rules["models"],
        json!({"ask": "haiku@low", "scribe": "haiku@medium"})
    );

    // An old live-scribe page still loads; its sections are ignored by v1.
    let library = FakeLibrary::initialized();
    library
        .write_document(
            Path::new("knowledge/domain/dom-scribe-rules.md"),
            "---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n---\n## Note\n- Decisions.\n\n## Skip\n- Status chatter.\n\n## Kinds\n- Borderline guidance.\n",
        )
        .unwrap();
    let old_ports = ports(library);
    let rules = pending(&old_ports, "short.jsonl")["rules"].clone();
    assert_eq!(rules["source"], "defaults");
    assert_eq!(rules["topics"], json!(["constellation"]));
    assert_eq!(rules["matters"], json!(["release-with-open-defects"]));
    assert_eq!(
        rules["models"],
        json!({"ask": "haiku@low", "scribe": "haiku@medium"})
    );
    let tags: Vec<&str> = rules["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(tags.contains(&"decision") && tags.contains(&"incident"));
}

#[test]
fn pending_refuses_other_providers_and_unsafe_session_ids() {
    let ports = ports(FakeLibrary::initialized());
    let response = call(
        &ports,
        "office.scribe-pending",
        json!({"session_id":SESSION,"transcript":"short.jsonl","provider":"codex","cwd":"/w"}),
    );
    assert_eq!(response["error"]["code"], "unsupported_provider");
    let response = call(
        &ports,
        "office.scribe-pending",
        json!({"session_id":"../x","transcript":"short.jsonl","provider":"claude-code","cwd":"/w"}),
    );
    assert_eq!(response["error"]["code"], "invalid_argument");
    let response = call(
        &ports,
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0001"}),
    );
    assert_eq!(response["error"]["code"], "scribe_not_parsed");
}

#[test]
fn append_creates_then_appends_merges_topics_advances_and_reruns_add_nothing() {
    let ports = ports(FakeLibrary::initialized());
    pending(&ports, "short.jsonl");
    let first = append(
        &ports,
        json!({"through":"m-0002","counterpart":"orchestrator","counterpart_name":"opus orchestrator","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"decision","topics":["constellation"],"refs":["ORB-20690"]},
        ]}),
    );
    assert_eq!(first["created"], true);
    assert_eq!(first["scribed_through"], "m-0002");
    assert_eq!(first["refused"], json!([]));
    let (path, text) = scr(&ports);
    assert_eq!(
        path,
        "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time.md"
    );
    assert_eq!(
        text,
        format!(
            "---\nid: \"SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time\"\ntitle: null\nsession_id: \"{SESSION}\"\nprovider: \"claude-code\"\nmodel: \"claude-opus-5-5\"\ncounterpart: \"orchestrator\"\ncounterpart_name: \"opus orchestrator\"\nhost: \"macbook\"\nproject: \"constellation\"\ncwd: \"/Users/daniel/workspace/constellation\"\nbranch: \"agent-main\"\ntranscript: \"short.jsonl\"\nparsed: \".orbit/tmp/scribe/{SESSION}.jsonl\"\nstarted_at: \"2026-10-31T20:07:00-07:00\"\nlast_scribed_at: \"2026-10-31T22:00-07:00\"\nscribed_through: \"m-0002\"\ntopics: [\"constellation\"]\naliases: []\n---\n- m-0001 2026-10-31T20:07-07:00 #decision #no-why #constellation \"lets go one step at a time\" refs: ORB-20690\n"
        )
    );
    assert_eq!(
        first["added"],
        json!([{"line":21,"text":"- m-0001 2026-10-31T20:07-07:00 #decision #no-why #constellation \"lets go one step at a time\" refs: ORB-20690"}])
    );
    let head = ports.library.head_commit().unwrap();

    // A second call appends after the first line and moves the cursor.
    pending(&ports, "long.jsonl");
    let second = append(
        &ports,
        json!({"through":"m-0006","entries":[
            {"msg":"m-0004","quote":"User message 4.","tag":"todo","topics":["constellation"],"due":"2026-11-01T15:00-07:00"},
            {"msg":"m-0005","quote":"reply 5","tag":"landed","topics":[]},
        ]}),
    );
    assert_eq!(second["created"], false);
    assert_eq!(second["previous_through"], "m-0002");
    assert_eq!(second["scribed_through"], "m-0006");
    let (_, text) = scr(&ports);
    let body: Vec<&str> = text.lines().skip(20).collect();
    assert_eq!(
        body,
        [
            "- m-0001 2026-10-31T20:07-07:00 #decision #no-why #constellation \"lets go one step at a time\" refs: ORB-20690",
            "- m-0004 2026-10-31T20:13-07:00 #todo #constellation \"User message 4.\" due: 2026-11-01T15:00-07:00",
            "- m-0005 2026-10-31T20:14-07:00 #landed \"reply 5\"",
        ]
    );
    assert!(text.contains("\nscribed_through: \"m-0006\"\n"));
    assert_ne!(ports.library.head_commit().unwrap(), head);

    // A re-run with no new messages adds nothing and commits nothing.
    let head = ports.library.head_commit().unwrap();
    let rerun = append(&ports, json!({"through":"m-0006","entries":[]}));
    assert_eq!(rerun["written"], false);
    assert_eq!(rerun["added"], json!([]));
    assert_eq!(ports.library.head_commit().unwrap(), head);
    assert_eq!(scr(&ports).1, text);
    // A cursor behind the file is refused.
    let response = call(
        &ports,
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0003"}),
    );
    assert_eq!(response["error"]["code"], "scribe_through_behind");
}

#[test]
fn topics_merge_from_entries_into_front_matter() {
    let library = FakeLibrary::initialized();
    library
        .write_document(
            Path::new("knowledge/domain/dom-scribe-rules.md"),
            "---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n---\n## Note\n- Decisions.\n\n## Topics\n- scribe\n- recall\n- orbit-office\n",
        )
        .unwrap();
    let ports = ports(library);
    pending(&ports, "short.jsonl");
    append(
        &ports,
        json!({"through":"m-0001","entries":[{"msg":"m-0001","quote":"Keep the SCR file grep-able.","tag":"direction","topics":["scribe","recall"]}]}),
    );
    let out = append(
        &ports,
        json!({"through":"m-0002","entries":[{"msg":"m-0002","quote":"Merged as 7266911","tag":"landed","topics":["orbit-office","scribe"],"refs":["PR#268"]}]}),
    );
    assert_eq!(out["topics"], json!(["scribe", "recall", "orbit-office"]));
    assert!(
        scr(&ports)
            .1
            .contains("\ntopics: [\"scribe\", \"recall\", \"orbit-office\"]\n")
    );
}

#[test]
fn append_refuses_and_never_writes_invalid_entries() {
    let ports = ports(FakeLibrary::initialized());
    pending(&ports, "long.jsonl");
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"msg":"m-0001","quote":"lets go two steps at a time","tag":"decision","topics":["constellation"]},
            {"msg":"m-0001","quote":"lets go","tag":"verdict","topics":["constellation"]},
            {"msg":"m-0001","quote":"lets go","tag":"decision","topics":["unlisted"]},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"decision","topics":[]},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"todo","topics":[],"by":"opus"},
            {"text":"ship the spec","tag":"direction","topics":[],"by":"opus"},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"agent-decision","topics":[]},
            {"msg":"m-0001","quote":"lets go","tag":"decision","topics":[],"by":"opus"},
            {"msg":"m-0001","quote":"lets go","tag":"pending","topics":[]},
            {"msg":"m-0002","quote":"Merged","tag":"resolved","topics":[],"by":"opus","resolves":"SCR-31-2007-nothing#L21"},
            {"msg":"m-0002","quote":"Merged","tag":"resolved","topics":[],"by":"opus"},
            {"msg":"m-0006","quote":"Assistant reply 7.","tag":"fact","topics":[]},
            {"msg":"m-0004","quote":"User message 4.","tag":"todo","topics":[]},
        ]}),
    );
    let reasons: Vec<(u64, &str)> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["index"].as_u64().unwrap(), r["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reasons,
        [
            (0, "quote_not_verbatim"),
            (1, "unknown_tag"),
            (2, "unknown_topic"),
            (3, "tag_not_permitted"),
            (4, "tag_not_permitted"),
            (5, "tag_not_permitted"),
            (6, "missing_by"),
            (7, "author_mismatch"),
            (8, "tag_not_permitted"),
            (9, "unresolved_pending"),
            (10, "missing_resolves"),
            (11, "after_through"),
        ]
    );
    // Only the last entry is valid; the refused ones are not in the file.
    let (_, text) = scr(&ports);
    let body: Vec<&str> = text.lines().skip(20).collect();
    assert_eq!(
        body,
        ["- m-0004 2026-10-31T20:13-07:00 #todo \"User message 4.\""]
    );

    // Context is read-only once scribed.
    let out = append(
        &ports,
        json!({"through":"m-0007","entries":[{"msg":"m-0002","quote":"Merged","tag":"fact","topics":[]}]}),
    );
    assert_eq!(out["refused"][0]["reason"], "context_message");
    // A malformed entry refuses the whole call.
    let response = call(
        &ports,
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0007","entries":[{"msg":"m-0007","tag":"fact"}]}),
    );
    assert_eq!(response["error"]["code"], "invalid_argument");
}

#[test]
fn notes_write_note_with_by_and_resolved_names_an_earlier_pending_line() {
    let ports = ports(FakeLibrary::initialized());
    pending(&ports, "long.jsonl");
    append(
        &ports,
        json!({"through":"m-0001","entries":[
            {"text":"buy a new SSD\nfor the server","tag":"todo","topics":[],"by":"user"},
            {"text":"waiting on CI for ORB-20780","tag":"pending","topics":["constellation"],"refs":["ORB-20780"],"by":"opus"},
        ]}),
    );
    let (path, text) = scr(&ports);
    let id = path.rsplit('/').next().unwrap().trim_end_matches(".md");
    assert_eq!(
        text.lines().skip(20).collect::<Vec<_>>(),
        [
            "- note 2026-10-31T22:00-07:00 #todo \"buy a new SSD\\nfor the server\" by: user",
            "- note 2026-10-31T22:00-07:00 #pending #constellation \"waiting on CI for ORB-20780\" refs: ORB-20780 by: opus",
        ]
    );
    let out = append(
        &ports,
        json!({"through":"m-0002","entries":[
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"resolved","topics":[],"by":"opus","resolves":format!("{id}#L22")},
            {"text":"CI is green","tag":"resolved","topics":[],"by":"opus","resolves":format!("{id}#L21")},
        ]}),
    );
    assert_eq!(out["refused"].as_array().unwrap().len(), 1);
    assert_eq!(out["refused"][0]["index"], 1);
    assert_eq!(out["refused"][0]["reason"], "unresolved_pending");
    assert_eq!(
        scr(&ports).1.lines().nth(22).unwrap(),
        format!(
            "- m-0002 2026-10-31T20:08-07:00 #resolved \"Merged as 7266911\" resolves: {id}#L22 by: opus"
        )
    );
}

#[test]
fn a_name_clash_takes_the_session_suffix_and_a_title_renames_once() {
    let library = FakeLibrary::initialized();
    let taken =
        "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time.md";
    library.insert_scribe_file(
        taken,
        "---\nid: SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time\nsession_id: other-session\naliases: []\n---\n",
    );
    let ports = ports(library);
    let pend = pending(&ports, "short.jsonl");
    let suffixed = "SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time-0a3104";
    assert_eq!(pend["scr"]["id"], suffixed);
    let out = append(&ports, json!({"through":"m-0001","entries":[]}));
    assert_eq!(out["id"], suffixed);

    let out = append(
        &ports,
        json!({"through":"m-0001","title":"Scribe v1 design","entries":[]}),
    );
    assert_eq!(out["renamed_from"], suffixed);
    assert_eq!(out["id"], "SCR-31-2007-scribe-v1-design");
    let files = ports.library.scribe_files().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design-lets-go-one-step-at-a-time.md",
            "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design.md",
        ]
    );
    let text = files[1].markdown.clone().unwrap();
    assert!(text.contains("\ntitle: \"Scribe v1 design\"\n"));
    assert!(text.contains(&format!("\naliases: [\"{suffixed}\"]\n")));

    // The same title again does not rename.
    let head = ports.library.head_commit().unwrap();
    let out = append(
        &ports,
        json!({"through":"m-0001","title":"Scribe v1 design","entries":[]}),
    );
    assert_eq!(out["renamed_from"], Value::Null);
    assert_eq!(out["written"], false);
    assert_eq!(ports.library.head_commit().unwrap(), head);
}

#[test]
fn decision_with_why_writes_why_and_without_why_gets_no_why() {
    let ports = ports(FakeLibrary::initialized());
    pending(&ports, "long.jsonl");
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"decision","topics":["constellation"],"why":{"msg":"m-0001","quote":"Keep the SCR file grep-able."}},
            {"msg":"m-0004","quote":"User message 4.","tag":"decision","topics":[]},
            {"msg":"m-0001","quote":"lets go","tag":"decision","topics":[],"why":{"msg":"m-0001","quote":"not in the message"}},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"agent-decision","topics":[],"by":"opus","why":{"msg":"m-0002","quote":"PR#268 is in."}},
            {"msg":"m-0005","quote":"reply 5","tag":"agent-decision","topics":[],"by":"opus"},
        ]}),
    );
    assert_eq!(out["refused"].as_array().unwrap().len(), 1);
    assert_eq!(out["refused"][0]["index"], 2);
    assert_eq!(out["refused"][0]["reason"], "why_not_verbatim");
    let (_, text) = scr(&ports);
    let body: Vec<&str> = text.lines().skip(20).collect();
    assert_eq!(
        body,
        [
            "- m-0001 2026-10-31T20:07-07:00 #decision #constellation \"lets go one step at a time\" why: m-0001 \"Keep the SCR file grep-able.\"",
            "- m-0004 2026-10-31T20:13-07:00 #decision #no-why \"User message 4.\"",
            "- m-0002 2026-10-31T20:08-07:00 #agent-decision \"Merged as 7266911\" why: m-0002 \"PR#268 is in.\" by: opus",
            "- m-0005 2026-10-31T20:14-07:00 #agent-decision #no-why \"reply 5\" by: opus",
        ]
    );
}

#[test]
fn why_note_resolves_an_existing_decision_line_only() {
    let ports = ports(FakeLibrary::initialized());
    pending(&ports, "long.jsonl");
    append(
        &ports,
        json!({"through":"m-0004","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"decision","topics":[]},
            {"msg":"m-0004","quote":"User message 4.","tag":"todo","topics":[]},
        ]}),
    );
    let (path, _) = scr(&ports);
    let id = path.rsplit('/').next().unwrap().trim_end_matches(".md");
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"text":"because the file must stay grep-able","tag":"why","topics":[],"by":"user","resolves":format!("{id}#L21")},
            {"text":"wrong target","tag":"why","topics":[],"by":"user","resolves":format!("{id}#L22")},
            {"text":"missing target","tag":"why","topics":[],"by":"opus","resolves":format!("{id}#L99")},
        ]}),
    );
    let reasons: Vec<&str> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, ["unresolved_decision", "unresolved_decision"]);
    assert_eq!(
        scr(&ports).1.lines().nth(22).unwrap(),
        format!(
            "- note 2026-10-31T22:00-07:00 #why \"because the file must stay grep-able\" resolves: {id}#L21 by: user"
        )
    );
}

#[test]
fn preference_requires_user_listed_matter_and_defaults_review_by() {
    let ports = ports(FakeLibrary::initialized());
    let pend = pending(&ports, "long.jsonl");
    assert_eq!(
        pend["rules"]["matters"],
        json!(["release-with-open-defects"])
    );
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"preference","topics":[],"matter":"release-with-open-defects"},
            {"msg":"m-0004","quote":"User message 4.","tag":"preference","topics":[],"matter":"release-with-open-defects","by":"opus"},
            {"msg":"m-0004","quote":"User message 4.","tag":"preference","topics":[],"matter":"unlisted-matter"},
            {"msg":"m-0004","quote":"User message 4.","tag":"preference","topics":[]},
            {"text":"always ship early","tag":"preference","topics":[],"by":"opus","matter":"release-with-open-defects"},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"preference","topics":[],"matter":"release-with-open-defects"},
        ]}),
    );
    let reasons: Vec<&str> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(
        reasons,
        [
            "author_mismatch",
            "unknown_matter",
            "missing_matter",
            "tag_not_permitted",
            "tag_not_permitted"
        ]
    );
    // m-0001 is 2026-10-31 local; 90 days later is 2027-01-29.
    assert_eq!(
        scr(&ports).1.lines().nth(20).unwrap(),
        "- m-0001 2026-10-31T20:07-07:00 #preference \"lets go one step at a time\" matter: release-with-open-defects review_by: 2027-01-29"
    );
    // An explicit review_by is kept.
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"text":"always review early","tag":"preference","topics":[],"by":"user","matter":"release-with-open-defects","review_by":"2027-06-01"},
        ]}),
    );
    assert_eq!(out["refused"], json!([]));
    assert!(
        scr(&ports)
            .1
            .contains("matter: release-with-open-defects review_by: 2027-06-01")
    );
}

#[test]
fn preference_supersedes_same_matter_only_and_preference_note_promotes_decision() {
    let library = FakeLibrary::initialized();
    library
        .write_document(
            Path::new("knowledge/domain/dom-scribe-rules.md"),
            "---\nid: dom-scribe-rules\ntype: domain\ntemplate: scribe-rules\ntitle: Scribe rules\n---\n## Matters\n- release-with-open-defects\n- other-matter\n",
        )
        .unwrap();
    let ports = ports(library);
    let pend = pending(&ports, "long.jsonl");
    assert_eq!(
        pend["rules"]["matters"],
        json!(["release-with-open-defects", "other-matter"])
    );
    append(
        &ports,
        json!({"through":"m-0004","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"decision","topics":[]},
            {"msg":"m-0004","quote":"User message 4.","tag":"preference","topics":[],"matter":"release-with-open-defects"},
        ]}),
    );
    let (path, _) = scr(&ports);
    let id = path.rsplit('/').next().unwrap().trim_end_matches(".md");
    // Decision is L21, first preference is L22.
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"msg":"m-0005","quote":"reply 5","tag":"preference","topics":[],"matter":"release-with-open-defects"},
            {"text":"promoted preference","tag":"preference","topics":[],"by":"user","matter":"other-matter","resolves":format!("{id}#L21")},
        ]}),
    );
    // m-0005 is an assistant message, so that preference attempt is refused
    // on authorship; the promotion note is accepted.
    assert_eq!(out["refused"].as_array().unwrap().len(), 1);
    assert_eq!(out["refused"][0]["reason"], "tag_not_permitted");
    assert!(
        scr(&ports)
            .1
            .contains(&format!("resolves: {id}#L21 by: user"))
    );
    // Superseding a preference on a different matter is refused.
    let out = append(
        &ports,
        json!({"through":"m-0005","entries":[
            {"text":"other preference","tag":"preference","topics":[],"by":"user","matter":"other-matter","supersedes":format!("{id}#L22")},
        ]}),
    );
    assert_eq!(out["refused"][0]["reason"], "invalid_supersedes");
}

#[test]
fn preference_supersedes_same_matter_and_refuses_non_preference() {
    // Supersedes: same matter accepted, different matter and non-preference refused.
    let ports2 = ports(FakeLibrary::initialized());
    pending(&ports2, "long.jsonl");
    append(
        &ports2,
        json!({"through":"m-0004","entries":[
            {"msg":"m-0001","quote":"lets go one step at a time","tag":"decision","topics":[]},
            {"msg":"m-0004","quote":"User message 4.","tag":"preference","topics":[],"matter":"release-with-open-defects"},
        ]}),
    );
    let (path2, _) = scr(&ports2);
    let id2 = path2.rsplit('/').next().unwrap().trim_end_matches(".md");
    let out = append(
        &ports2,
        json!({"through":"m-0005","entries":[
            {"text":"ship early from now on","tag":"preference","topics":[],"by":"user","matter":"release-with-open-defects","supersedes":format!("{id2}#L22")},
            {"text":"wrong matter","tag":"preference","topics":[],"by":"user","matter":"release-with-open-defects","supersedes":format!("{id2}#L21")},
        ]}),
    );
    assert_eq!(out["refused"].as_array().unwrap().len(), 1);
    assert_eq!(out["refused"][0]["index"], 1);
    assert_eq!(out["refused"][0]["reason"], "invalid_supersedes");
    assert!(scr(&ports2).1.contains(&format!("supersedes: {id2}#L22")));
}
