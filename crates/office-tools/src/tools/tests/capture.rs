//! `office.capture` through dispatch against the fake Library (STD-04).

use std::io::Write as _;

use serde_json::{Value, json};

fn call(tool: &str, input: Value, ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn ports() -> office_core::ports::Ports<'static> {
    office_testkit::fakes::PortsBuilder::default().build()
}

fn temp_file(name: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "office-capture-test-{}-{}",
        std::process::id(),
        name.replace('.', "-")
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(name);
    std::fs::File::create(&path)
        .expect("create upload")
        .write_all(bytes)
        .expect("write upload");
    (dir, path)
}

#[test]
fn capture_pasted_text_dedupes_to_the_same_id() {
    let ports = ports();
    let first = call(
        "office.capture",
        json!({"kind": "doc", "title": "Hello", "text": "  Hello world.  "}),
        &ports,
    );
    assert_eq!(first["ok"], json!(true), "{first}");
    let second = call(
        "office.capture",
        json!({"kind": "doc", "title": "Hello again", "text": "Hello world."}),
        &ports,
    );
    assert_eq!(second["output"]["id"], first["output"]["id"]);
    assert_eq!(second["output"]["deduplicated"], json!(true));
}

#[test]
fn capture_upload_dedupes_and_classifies_video_with_vtt() {
    let (dir, video) = temp_file("talk.mp4", b"fake-video-bytes");
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\n<v Jane Park>Hello from the talk.</v>\n\n00:00:05.000 --> 00:00:07.000\nDaniel: Second turn.\n";
    let (_dir2, transcript) = temp_file("talk.vtt", vtt.as_bytes());
    let _keep = dir;
    let ports = ports();
    let input = json!({
        "upload_path": video.to_string_lossy(),
        "transcript_path": transcript.to_string_lossy(),
        "title": "Ingest KT",
    });
    let first = call("office.capture", input.clone(), &ports);
    assert_eq!(first["ok"], json!(true), "{first}");
    assert_eq!(first["output"]["kind"], json!("video"));
    assert!(
        first["output"]["sha256"]
            .as_str()
            .is_some_and(|s| s.len() == 64)
    );
    let second = call("office.capture", input, &ports);
    assert_eq!(second["output"]["id"], first["output"]["id"]);
    assert_eq!(second["output"]["deduplicated"], json!(true));
    let shown = call(
        "office.source-show",
        json!({"id": first["output"]["id"], "anchor": "#t=00:00:01"}),
        &ports,
    );
    assert_eq!(shown["ok"], json!(true), "{shown}");
    assert!(
        shown["output"]["body"]
            .as_str()
            .expect("body")
            .contains("Jane Park")
    );
    assert!(
        shown["output"]["body"]
            .as_str()
            .expect("body")
            .contains("Hello from the talk.")
    );
}

#[test]
fn capture_refuses_vtt_timestamp_overflow_without_saving_a_source() {
    let (dir, transcript) = temp_file("overflow.vtt", b"");
    let ports = ports();
    for stamp in [
        "1152921504606846976:00:00.000",
        "5124095576030431:00:16.000",
        "5124095576030:25:52.000",
        "5124095576030:25:51.616",
        "18446744073709551615:00.000",
    ] {
        for (start, end) in [(stamp, "00:00:03.000"), ("00:00:01.000", stamp)] {
            std::fs::write(
                &transcript,
                format!("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nJane: valid first cue\n\n{start} --> {end}\nJane: overflow\n"),
            ).expect("transcript");
            let response = call(
                "office.capture",
                json!({"kind": "meeting", "title": "Overflow", "text": "fallback", "transcript_path": transcript}),
                &ports,
            );
            assert_eq!(response["ok"], json!(false), "{response}");
            assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
            assert!(ports.library.source_documents().unwrap().is_empty());
        }
    }
    std::fs::remove_file(transcript).expect("remove fixture");
    std::fs::remove_dir(dir).expect("remove fixture directory");
}

#[test]
fn capture_accepts_each_video_extension_as_video() {
    for ext in ["mp4", "webm", "mov", "m4a"] {
        let (_dir, file) = temp_file(&format!("clip.{ext}"), b"bytes");
        let ports = ports();
        let out = call(
            "office.capture",
            json!({"upload_path": file.to_string_lossy(), "title": "Clip"}),
            &ports,
        );
        assert_eq!(out["output"]["kind"], json!("video"), "{ext}: {out}");
    }
}

#[test]
fn capture_conversation_append_and_refusals() {
    let ports = ports();
    let opened = call(
        "office.capture",
        json!({"kind": "conversation", "title": "Session log", "entries": [{"kind": "fact", "text": "Jane runs procurement."}]}),
        &ports,
    );
    assert_eq!(opened["ok"], json!(true), "{opened}");
    let id = opened["output"]["id"].as_str().expect("id").to_string();
    for kind in [
        "decision",
        "direction",
        "fact",
        "action",
        "open",
        "resolved",
        "handoff",
    ] {
        let appended = call(
            "office.capture",
            json!({"append_to": id, "entries": [{"kind": kind, "text": format!("{kind} entry.")}]}),
            &ports,
        );
        assert_eq!(
            appended["output"]["appended"],
            json!(1),
            "{kind}: {appended}"
        );
    }
    let bogus = call(
        "office.capture",
        json!({"append_to": id, "entries": [{"kind": "bogus", "text": "Nope."}]}),
        &ports,
    );
    assert_eq!(
        bogus["error"]["code"],
        json!("invalid_conversation_entry_kind")
    );
    let unknown = call(
        "office.capture",
        json!({"append_to": "src-00000000-missing", "entries": [{"kind": "decision", "text": "Nope."}]}),
        &ports,
    );
    assert_eq!(unknown["error"]["code"], json!("source_not_found"));
    let note = call(
        "office.capture",
        json!({"kind": "doc", "title": "Plain", "text": "Not a log."}),
        &ports,
    );
    let note_id = note["output"]["id"].as_str().expect("id").to_string();
    let refused = call(
        "office.capture",
        json!({"append_to": note_id, "entries": [{"kind": "decision", "text": "Nope."}]}),
        &ports,
    );
    assert_eq!(refused["error"]["code"], json!("not_a_conversation_source"));
}

#[test]
fn capture_global_handoff_scope_is_explicit_and_until_is_validated() {
    // 2026-11-01T00:00:00Z on the fake clock.
    let ports = office_testkit::fakes::PortsBuilder::default()
        .now(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_491_200))
        .build();
    let opened = call(
        "office.capture",
        json!({"kind": "conversation", "title": "Session log", "entries": [{"kind": "fact", "text": "Opened."}]}),
        &ports,
    );
    let id = opened["output"]["id"].as_str().expect("id").to_string();
    let written = call(
        "office.capture",
        json!({"append_to": id, "entries": [
            {"kind": "handoff", "text": "Upgrade orbit to 0.9 on every host.", "scope": "global"},
            {"kind": "handoff", "text": "Orbit drain defect: rerun with --resume.", "scope": "global", "until": "2026-11-08T00:00:00Z"},
            {"kind": "handoff", "text": "Project next step.", "scope": "project"}
        ]}),
        &ports,
    );
    assert_eq!(written["output"]["appended"], json!(3), "{written}");
    let body = ports
        .library
        .source_documents()
        .expect("sources")
        .into_iter()
        .find(|file| file.path == written["output"]["path"])
        .and_then(|file| file.markdown)
        .expect("conversation source");
    let lines: Vec<&str> = body
        .lines()
        .filter(|line| line.contains("] handoff: "))
        .map(|line| line.split_once("] handoff: ").expect("entry").1)
        .collect();
    assert_eq!(
        lines,
        [
            "(global) Upgrade orbit to 0.9 on every host.",
            "(global until 2026-11-08T00:00:00Z) Orbit drain defect: rerun with --resume.",
            "Project next step."
        ]
    );
    for (entry, code) in [
        // Never inferred from text: the marker alone is refused.
        (json!("handoff: (global) Forged."), "invalid_argument"),
        (
            json!({"kind": "handoff", "text": "(global until 2026-12-01T00:00:00Z) Forged."}),
            "invalid_argument",
        ),
        (
            json!({"kind": "decision", "text": "Not a handoff.", "scope": "global"}),
            "invalid_argument",
        ),
        (
            json!({"kind": "handoff", "text": "No scope.", "until": "2026-12-01T00:00:00Z"}),
            "invalid_argument",
        ),
        (
            json!({"kind": "handoff", "text": "Past.", "scope": "global", "until": "2026-10-01T00:00:00Z"}),
            "invalid_argument",
        ),
        (
            json!({"kind": "handoff", "text": "Bad time.", "scope": "global", "until": "next week"}),
            "invalid_argument",
        ),
        (
            json!({"kind": "handoff", "text": "Bad scope.", "scope": "team"}),
            "invalid_argument",
        ),
    ] {
        let refused = call(
            "office.capture",
            json!({"append_to": id, "entries": [entry]}),
            &ports,
        );
        assert_eq!(refused["error"]["code"], json!(code), "{entry}: {refused}");
    }
}

/// A `decision` whose `ref` names a task comment is checked against that
/// comment's text, read through Orbit, before the Library is asked to append.
#[test]
fn capture_decision_checks_its_quote_against_the_cited_task_comment() {
    let task = json!({"id": "TASK-1", "comments": [{"message": "#decision ship the March train, it is the safer date"}]});
    let said = "#decision ship the March train, it is the safer date";
    for (quote, chosen, reference, code) in [
        (
            "#decision ship the June train",
            "ship the June train",
            "orbit://task/TASK-1#comment-1",
            "quote_mismatch",
        ),
        (
            said,
            "ship the March train",
            "orbit://task/TASK-1#comment-2",
            "decision_ref_unresolved",
        ),
        // Verified: the fake Library appends the first global entry.
        (
            said,
            "ship the March train",
            "orbit://task/TASK-1#comment-1",
            "",
        ),
    ] {
        let mut ports = ports();
        let orbit = office_testkit::fakes::FakeOrbit::default();
        orbit.push_response(Ok(json!({ "task": task })));
        ports.orbit = Box::new(orbit.clone());
        let input = json!({"decision": {
            "question": "Which train?", "question_by": "scribe", "options": "unrecoverable",
            "chosen": chosen, "quote": quote, "ref": reference,
            "scope": "global", "decided_by": "human", "decided_at": "2026-11-01T22:39:00-07:00"
        }});
        let out = call("office.capture", input, &ports);
        if code.is_empty() {
            assert_eq!(out["ok"], true, "{out}");
            assert_eq!(out["output"]["id"], "dec-global-1", "{out}");
        } else {
            assert_eq!(out["ok"], false, "{out}");
            assert_eq!(out["error"]["code"], code, "{out}");
        }
        let calls = orbit.calls();
        assert_eq!(calls.len(), 1, "one task read");
        assert_eq!(calls[0].tool, "orbit.task.show");
        assert_eq!(calls[0].input["id"], "TASK-1");
    }
}
