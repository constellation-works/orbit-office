//! `office.scribe-ask` through public exec dispatch, over fixture SCR
//! files in an in-memory Library, with a fake model through the Models
//! port (specs/scribe.md, "Answering").
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use office_core::ports::{Ports, Sensitivity};
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels},
};
use serde_json::{Value, json};

const ID_A: &str = "SCR-31-2007-scribe-v1-design";
const PATH_A: &str = "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design.md";
const ID_P: &str = "SCR-30-0900-private-session";
const PATH_P: &str = "sources/conversation/2026/10/SCR-30-0900-private-session.md";

fn scr(id: &str, session: &str, entries: &[String]) -> String {
    scr_with(id, session, "", entries)
}

fn scr_with(id: &str, session: &str, extra_front: &str, entries: &[String]) -> String {
    let mut text = format!(
        "---\nid: \"{id}\"\ntitle: null\nsession_id: \"{session}\"\nprovider: \"claude-code\"\nmodel: null\ncounterpart: \"orchestrator\"\ncounterpart_name: null\nhost: \"macbook\"\nproject: \"constellation\"\ncwd: \"/work/constellation\"\nbranch: \"agent-main\"\ntranscript: \"t.jsonl\"\nparsed: \"library/.office/state/scribe/{session}.jsonl\"\nstarted_at: \"2026-10-29T09:00:00-07:00\"\nlast_scribed_at: null\nscribed_through: \"m-0002\"\ntopics: [\"scribe\"]\naliases: []\n{extra_front}---\n"
    );
    for entry in entries {
        text.push_str(entry);
        text.push('\n');
    }
    text
}

fn fixture_library() -> FakeLibrary {
    let library = FakeLibrary::initialized();
    let a = scr(
        ID_A,
        "session-a",
        &[
            "- m-0001 2026-10-31T10:00-07:00 #decision #scribe \"lets go one step at a time\" refs: ORB-20690".into(),
            "- m-0002 2026-10-31T11:00-07:00 #landed #recall \"merged as 7266911\" refs: PR#268".into(),
        ],
    );
    library.insert_scribe_file(PATH_A, &a);
    library
}

fn ports(library: FakeLibrary) -> (Ports<'static>, FakeModels) {
    let models = FakeModels::new();
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.models = Box::new(models.clone());
    (ports, models)
}

fn ask(ports: &Ports<'_>, input: Value) -> Value {
    let request =
        office_core::protocol::parse(&envelope::request("office.scribe-ask", input, json!({})))
            .unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

#[test]
fn answers_with_citations_using_fake_model_and_shortlist_makes_one_call() {
    let (ports, models) = ports(fixture_library());
    models.extract_replies.borrow_mut().push_back(json!({
        "not_captured": false,
        "claims": [{"text": "Daniel decided to go one step at a time.", "quote": "lets go one step at a time", "cites": [format!("{ID_A}#L21")]}]
    }));
    let response = ask(&ports, json!({"question": "one step at a time decision"}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    assert_eq!(out["not_captured"], false);
    assert_eq!(out["status"], "answered");
    assert_eq!(
        out["claims"][0]["text"],
        "Daniel decided to go one step at a time."
    );
    assert_eq!(out["claims"][0]["quote"], "lets go one step at a time");
    assert_eq!(out["claims"][0]["path"], PATH_A);
    assert_eq!(out["claims"][0]["line"], 21);
    assert_eq!(out["claims"][0]["file_line"], format!("{PATH_A}:21"));
    assert_eq!(out["claims"][0]["message_id"], "m-0001");
    assert_eq!(out["claims"][0]["msg"], "m-0001");
    assert_eq!(out["claims"][0]["content_withheld"], false);
    assert_eq!(out["dropped_claims"], 0);
    assert_eq!(
        out["derived_by"],
        json!({"route": "hosted", "model": "fake-hosted"})
    );
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 1, "exactly one model call");
    assert_eq!(requests[0].tier, office_core::ports::ModelTier::Standard);
    assert_eq!(requests[0].sensitivity, Sensitivity::Normal);
    assert!(requests[0].text.contains("lets go one step at a time"));
}

#[test]
fn misquoted_claim_is_dropped_and_uncovered_question_is_not_captured_with_range() {
    let (ports, models) = ports(fixture_library());
    models.extract_replies.borrow_mut().push_back(json!({
        "not_captured": false,
        "claims": [{"text": "Fabricated.", "quote": "this was never said", "cites": [format!("{ID_A}#L21")]}]
    }));
    let response = ask(&ports, json!({"question": "one step decision"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["claims"], json!([]));
    assert_eq!(response["output"]["dropped_claims"], 1);
    assert_eq!(response["output"]["not_captured"], true);

    let calls_before = models.extract_requests.borrow().len();
    let response = ask(
        &ports,
        json!({"question": "kubernetes cluster sizing", "since": "2026-10-01", "until": "2026-10-31"}),
    );
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    assert_eq!(out["not_captured"], true);
    assert_eq!(out["status"], "not_captured");
    assert_eq!(
        out["searched"]["date_range"],
        json!({"since": "2026-10-01", "until": "2026-10-31"})
    );
    assert_eq!(out["since"], "2026-10-01");
    assert_eq!(out["until"], "2026-10-31");
    assert_eq!(out["searched"]["sessions"][0]["session_id"], "session-a");
    assert_eq!(out["searched"]["sessions"][0]["scribed_through"], "m-0002");
    assert_eq!(
        models.extract_requests.borrow().len(),
        calls_before,
        "no shortlist, no model call"
    );
}

#[test]
fn private_entries_are_cited_by_reference_only_without_open_weights_route() {
    let library = FakeLibrary::initialized();
    let p = scr_with(
        ID_P,
        "session-p",
        "sensitivity: \"private\"\n",
        &["- m-0001 2026-10-30T10:00-07:00 #decision #scribe \"secret roadmap decision approved\"".into()],
    );
    library.insert_scribe_file(PATH_P, &p);
    let (ports, models) = ports(library);
    let response = ask(&ports, json!({"question": "secret roadmap decision"}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    assert_eq!(out["status"], "reference_only");
    assert_eq!(out["not_captured"], true);
    assert_eq!(out["private_references"][0]["scr_id"], ID_P);
    assert_eq!(out["private_references"][0]["content_withheld"], true);
    assert_eq!(out["private_references"][0]["message_id"], "m-0001");
    assert!(
        models.extract_requests.borrow().is_empty(),
        "private text sent to no model"
    );
    assert!(
        !response
            .to_string()
            .contains("secret roadmap decision approved"),
        "private quote never returned: {response}"
    );
}

#[test]
fn unknown_keys_and_missing_question_are_refused() {
    let (ports, _models) = ports(fixture_library());
    for input in [
        json!({"question": "x", "invent": true}),
        json!({"session_id": "s"}),
    ] {
        let response = ask(&ports, input);
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
}

#[test]
fn conformance_golden_runs_through_public_dispatch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/scribe-ask.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let (ports, models) = ports(FakeLibrary::initialized());
                let response = ask(&ports, input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                assert!(models.extract_requests.borrow().is_empty());
                count += 1;
            }
        }
    }
    assert_eq!(count, 3, "all golden cases executed");
}
