//! `office.recall-answer` through public exec dispatch, with a fixture
//! Library and a fixture Models adapter.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::path::Path;

use office_core::ports::{Library, Ports, Sensitivity};
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels, FakeOrbit},
};
use serde_json::{Value, json};

fn call(ports: &Ports<'_>, input: Value) -> Value {
    let request =
        office_core::protocol::parse(&envelope::request("office.recall-answer", input, json!({})))
            .unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn source(library: &FakeLibrary, id: &str, title: &str, private: bool, body: &str, refs: &str) {
    library
        .write_document(
            Path::new(&format!("sources/conversation/2026/10/{id}/source.md")),
            &format!(
                "---\nid: {id}\nkind: conversation\ntitle: {title}\nproject: /work/office\nsensitivity: {}\ncaptured_at: \"2026-10-28T00:00:00Z\"\nstarted_at: \"2026-10-28T09:00:00Z\"\nsession_id: sess-{id}\nresume: {{ provider: claude, command: \"claude --resume sess-{id}\" }}\nentry_refs: {refs}\n---\n{body}",
                if private { "private" } else { "normal" }
            ),
        )
        .unwrap();
}

/// A public decision with a task ref, and a private source on the same topic.
fn library() -> FakeLibrary {
    let library = FakeLibrary::initialized();
    source(
        &library,
        "src-pub",
        "Release planning",
        false,
        "[2026-10-28T10:00:00Z] decision: Ship the release on Friday.\n[2026-10-28T10:05:00Z] action: Opened ORB-7 for the release.\n",
        "{ L2: [{ kind: task, id: ORB-7 }] }",
    );
    source(
        &library,
        "src-priv",
        "Release finances",
        true,
        "[2026-10-28T11:00:00Z] fact: PRIVATE RELEASE BUDGET\n",
        "{}",
    );
    library
}

fn fixture(library: FakeLibrary) -> (Ports<'static>, FakeOrbit, FakeModels) {
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    let orbit = FakeOrbit::default();
    let models = FakeModels::new();
    ports.library = Box::new(library);
    ports.orbit = Box::new(orbit.clone());
    ports.models = Box::new(models.clone());
    (ports, orbit, models)
}

fn open_weights(library: &FakeLibrary) {
    library
        .set_setting(
            "open_weights",
            json!({"endpoint":"https://private.example.com/v1","model":"private-model","kind":"company","label":"Company"}),
        )
        .unwrap();
}

#[test]
fn answer_cites_anchors_names_the_conversation_reads_live_refs_and_lists_private_by_reference() {
    let library = library();
    let (ports, orbit, models) = fixture(library);
    let before = ports.library.head_commit().unwrap();
    orbit.push_response(Ok(json!({"id":"ORB-7","status":"done"})));
    models.extract_replies.borrow_mut().push_back(json!({
        "not_captured": false,
        "claims": [
            {"text": "The release ships on Friday.", "cites": ["src-pub#L1"]},
            {"text": "ORB-7 tracks it.", "cites": ["src-pub#L2"]},
            {"text": "Invented from memory.", "cites": ["src-pub#L99"]},
            {"text": "Leaks the budget.", "cites": ["src-priv#L1"]}
        ]
    }));
    let response = call(&ports, json!({"question": "release"}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    let answer = &out["answer"];
    assert_eq!(answer["status"], "answered");
    assert_eq!(
        answer["claims"],
        json!([
            {"text": "The release ships on Friday.", "cites": ["src-pub#L1"], "content_withheld": false},
            {"text": "ORB-7 tracks it.", "cites": ["src-pub#L2"], "content_withheld": false}
        ])
    );
    assert_eq!(
        answer["dropped_claims"], 2,
        "dangling and unsent cites are dropped"
    );
    assert_eq!(answer["conversations"][0]["source_id"], "src-pub");
    assert_eq!(
        answer["conversations"][0]["resume"]["command"],
        "claude --resume sess-src-pub"
    );
    assert_eq!(
        answer["refs"],
        json!([{"kind":"task","id":"ORB-7","status":"done","workspace":null}])
    );
    assert_eq!(answer["private_references"][0]["source_id"], "src-priv");
    assert_eq!(answer["private_references"][0]["title"], "Release finances");
    assert_eq!(answer["coverage"]["private_sessions_skipped"], 1);
    let text = answer["text"].as_str().unwrap();
    for expected in [
        "The release ships on Friday. [src-pub#L1]",
        "Release planning (/work/office, 2026-10-28) — resume: claude --resume sess-src-pub",
        "ORB-7 is done",
        "Private conversation (by reference): Release finances [src-priv]",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert_eq!(out["recall"]["entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        out["derived_by"],
        json!({"route":"hosted","model":"fake-hosted"})
    );

    // One Standard-tier call, on the hosted route, without the private text.
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].tier, office_core::ports::ModelTier::Standard);
    assert_eq!(requests[0].sensitivity, Sensitivity::Normal);
    assert!(requests[0].text.contains("Ship the release on Friday."));
    assert!(requests[0].text.contains("src-pub#L1"));
    assert!(!requests[0].text.contains("PRIVATE RELEASE BUDGET"));
    assert!(!response.to_string().contains("PRIVATE RELEASE BUDGET"));
    assert_eq!(ports.library.head_commit().unwrap(), before, "read-only");
}

#[test]
fn private_entries_go_to_the_model_only_on_an_open_weights_route_and_return_withheld() {
    let library = library();
    open_weights(&library);
    let (ports, orbit, models) = fixture(library);
    orbit.push_response(Ok(json!({"id":"ORB-7","status":"review"})));
    models.extract_replies.borrow_mut().push_back(json!({
        "not_captured": false,
        "claims": [
            {"text": "The release ships on Friday.", "cites": ["src-pub#L1"]},
            {"text": "The budget is PRIVATE RELEASE BUDGET.", "cites": ["src-priv#L1"]}
        ]
    }));
    let response = call(&ports, json!({"question": "release"}));
    assert_eq!(response["ok"], true, "{response}");
    let answer = &response["output"]["answer"];
    assert_eq!(
        answer["claims"][1],
        json!({"text": null, "cites": ["src-priv#L1"], "content_withheld": true})
    );
    assert_eq!(
        answer["private_references"],
        json!([]),
        "routed private sources are evidence, not references"
    );
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("Private evidence (content withheld) [src-priv#L1]")
    );
    assert!(!response.to_string().contains("PRIVATE RELEASE BUDGET"));
    assert_eq!(
        response["output"]["derived_by"],
        json!({"route":"private:company","model":"fake-private"})
    );
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].sensitivity,
        Sensitivity::Private,
        "routed to open weights"
    );
    assert!(requests[0].text.contains("PRIVATE RELEASE BUDGET"));
}

#[test]
fn no_evidence_is_not_captured_with_coverage_and_only_private_evidence_is_reference_only() {
    let (ports, _orbit, models) = fixture(library());
    let response = call(&ports, json!({"question": "kubernetes"}));
    assert_eq!(response["ok"], true, "{response}");
    let answer = &response["output"]["answer"];
    assert_eq!(answer["status"], "not_captured");
    assert!(answer["text"].as_str().unwrap().starts_with("Not captured"));
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("capture reaches back to 2026-10-28T09:00:00Z")
    );
    assert_eq!(response["output"]["derived_by"], Value::Null);

    let response = call(&ports, json!({"question": "budget"}));
    let answer = &response["output"]["answer"];
    assert_eq!(answer["status"], "reference_only");
    assert_eq!(answer["private_references"][0]["source_id"], "src-priv");
    assert!(
        models.extract_requests.borrow().is_empty(),
        "no evidence, no model call"
    );

    // A reply whose claims are all unsupported is not an answer.
    models
        .extract_replies
        .borrow_mut()
        .push_back(json!({"not_captured": false, "claims": [{"text": "Guess.", "cites": []}]}));
    let response = call(&ports, json!({"question": "friday"}));
    assert_eq!(response["output"]["answer"]["status"], "not_captured");
    assert_eq!(response["output"]["answer"]["dropped_claims"], 1);
    assert_eq!(models.extract_requests.borrow().len(), 1);

    // A malformed reply is a model error, never an empty success.
    models
        .extract_replies
        .borrow_mut()
        .push_back(json!({"answer": "free text"}));
    assert_eq!(
        call(&ports, json!({"question": "friday"}))["error"]["code"],
        "model_response_invalid"
    );
}

#[test]
fn conformance_golden_runs_through_public_dispatch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/recall-answer.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let (ports, _, models) = fixture(FakeLibrary::initialized());
                let response = call(&ports, input.take().unwrap());
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
    assert_eq!(count, 4, "all golden cases executed");
}
