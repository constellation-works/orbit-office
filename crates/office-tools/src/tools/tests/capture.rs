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
        json!({"kind": "note", "title": "Hello", "text": "  Hello world.  "}),
        &ports,
    );
    assert_eq!(first["ok"], json!(true), "{first}");
    let second = call(
        "office.capture",
        json!({"kind": "note", "title": "Hello again", "text": "Hello world."}),
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
        json!({"kind": "note", "title": "Plain", "text": "Not a log."}),
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
