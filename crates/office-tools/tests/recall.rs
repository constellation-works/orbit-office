//! Recall through public exec dispatch with recording ports and fixture documents.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::path::Path;

use office_core::ports::{Library, Ports};
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels, FakeOrbit},
};
use serde_json::{Value, json};

fn call(ports: &Ports<'_>, input: Value) -> Value {
    let request =
        office_core::protocol::parse(&envelope::request("office.recall", input, json!({})))
            .unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}
fn source(
    library: &FakeLibrary,
    id: &str,
    title: &str,
    project: &str,
    private: bool,
    body: &str,
    refs: Value,
) {
    fn flow(value: &Value) -> String {
        match value {
            Value::Object(map) => format!(
                "{{ {} }}",
                map.iter()
                    .map(|(k, v)| format!("{k}: {}", flow(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Array(items) => format!(
                "[{}]",
                items.iter().map(flow).collect::<Vec<_>>().join(", ")
            ),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
        }
    }
    let refs = flow(&refs);
    library.write_document(Path::new(&format!("sources/conversation/2026/10/{id}/source.md")), &format!(
        "---\nid: {id}\nkind: conversation\ntitle: {title}\nproject: {project}\nsensitivity: {}\ncaptured_at: \"2026-10-28T00:00:00Z\"\nentry_refs: {refs}\n---\n{body}", if private { "private" } else { "normal" }
    )).unwrap();
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

#[test]
fn recall_matches_titles_text_and_stored_refs_orders_by_instant_and_reads_live_status() {
    let library = FakeLibrary::initialized();
    // Offset time strings must sort by instant; blank body lines count in anchors.
    source(
        &library,
        "src-old",
        "Launch",
        "/work/a",
        false,
        "[2026-10-28T12:00:00+02:00] decision: Ship the release.\n\n[2026-10-28T10:00:00.500Z] action: Recorded change.\n",
        json!({"L1":[{"kind":"task","id":"DANI-10"}],"L3":[{"kind":"pr","id":"acme/office#12"}]}),
    );
    source(
        &library,
        "src-new",
        "Launch",
        "/work/b",
        false,
        "[2026-10-28T10:00:00.750Z] direction: Keep exact  spacing.\n",
        json!({"L1":[{"kind":"task","id":"DANI-10"},{"kind":"task","id":"ORB-11"},{"kind":"commit","id":"deadbeef"}]}),
    );
    source(
        &library,
        "src-private",
        "Launch",
        "/work/a",
        true,
        "[2026-10-28T10:01:00Z] fact: PRIVATE BODY MUST NOT ESCAPE\n",
        json!({"L1":[{"kind":"pr","id":"acme/office#99"}]}),
    );
    let (ports, orbit, models) = fixture(library);
    let before = ports.library.head_commit().unwrap();
    orbit.push_response(Ok(
        json!({"id":"DANI-10","status":"review","workspace":"daniel"}),
    ));
    orbit.push_response(Err(office_core::Error::OrbitUnavailable));
    let response = call(&ports, json!({"question":"launch"}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    assert_eq!(out["entries"].as_array().unwrap().len(), 3);
    assert_eq!(out["entries"][0]["source_id"], "src-new");
    assert_eq!(out["entries"][0]["text"], "Keep exact  spacing.");
    assert_eq!(out["entries"][1]["anchor"], "#L3");
    assert_eq!(out["sessions"].as_array().unwrap().len(), 3);
    assert!(
        !response
            .to_string()
            .contains("PRIVATE BODY MUST NOT ESCAPE")
    );
    let refs = out["refs"].as_array().unwrap();
    assert_eq!(refs.len(), 5);
    assert!(
        refs.iter()
            .any(|r| r["id"] == "DANI-10" && r["status"] == "review" && r["workspace"] == "daniel")
    );
    assert!(refs.iter().any(|r| r["id"] == "ORB-11"
        && r["status"] == "unresolved"
        && r["reason"] == "task_unavailable"));
    assert!(
        refs.iter()
            .any(|r| r["id"] == "acme/office#12" && r["reason"] == "no_pr_source")
    );
    assert_eq!(
        out["coverage"],
        json!({"earliest_session_at":"2026-10-28T00:00:00Z","private_sessions_skipped":1,"providers_not_captured":["codex"]})
    );
    assert_eq!(
        orbit.calls().len(),
        2,
        "one task read per distinct task identity"
    );
    assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.show"));
    assert_eq!(
        ports.library.head_commit().unwrap(),
        before,
        "recall is observational"
    );
    assert!(models.sensitivities().is_empty());
    assert!(models.extract_requests.borrow().is_empty());
    assert!(models.transcribe_requests.borrow().is_empty());
    assert_eq!(models.checks(), 0);
    // Status is not cached in the Library or the handler between calls.
    orbit.push_response(Ok(
        json!({"id":"DANI-10","status":"done","workspace":"daniel"}),
    ));
    let response = call(&ports, json!({"question":"DANI-10","project":"/work/a"}));
    assert_eq!(response["output"]["refs"][0]["status"], "done");
    assert_eq!(response["output"]["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(ports.library.head_commit().unwrap(), before);
}

#[test]
fn recall_filters_are_inclusive_limits_are_bounded_and_legacy_sources_are_readable() {
    let library = FakeLibrary::initialized();
    source(
        &library,
        "src-a",
        "Find this",
        "work-a",
        false,
        "[2026-10-28T23:59:59.999Z] fact: Older entry.\n[2026-10-29T00:00:00Z] fact: Newer entry.\n",
        json!({}),
    );
    let (ports, orbit, _) = fixture(library);
    let response = call(
        &ports,
        json!({"question":"FIND entry","since":"2026-10-28","until":"2026-10-28","project":"work-a"}),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(response["output"]["entries"][0]["text"], "Older entry.");
    assert!(response["output"]["sessions"][0]["resume"].is_null());
    assert_eq!(
        call(&ports, json!({"question":"find","limit":1}))["output"]["entries"][0]["text"],
        "Newer entry."
    );
    for input in [
        json!({}),
        json!({"question":" "}),
        json!({"question":"a","limit":0}),
        json!({"question":"a","limit":51}),
        json!({"question":"a","since":"bad"}),
        json!({"question":"a","since":"2026-10-29","until":"2026-10-28"}),
        json!({"question":"a","unknown":true}),
    ] {
        assert_eq!(call(&ports, input)["error"]["code"], "invalid_argument");
    }
    assert!(orbit.calls().is_empty());
}

#[test]
fn unavailable_and_malformed_task_statuses_never_fail_recall_or_echo_callback_errors() {
    let library = FakeLibrary::initialized();
    source(
        &library,
        "src-a",
        "Topic",
        "work",
        false,
        "[2026-10-28T10:00:00Z] action: Ship.\n",
        json!({"L1":[{"kind":"task","id":"ORB-42"}]}),
    );
    let (ports, orbit, _) = fixture(library);
    for reply in [
        Err(office_core::Error::InvalidArgument(
            "private callback detail".into(),
        )),
        Ok(json!({"id":"ORB-99","status":"done"})),
        Ok(json!({"id":"ORB-42","status":"private callback detail"})),
        Ok(json!({"id":"ORB-42"})),
    ] {
        orbit.push_response(reply);
        let response = call(&ports, json!({"question":"topic"}));
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["output"]["refs"][0]["status"], "unresolved");
        assert!(
            !response["output"]["refs"][0]["reason"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert!(!response.to_string().contains("private callback detail"));
    }
}

#[test]
fn conformance_golden_runs_through_public_dispatch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/recall.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let (ports, _, _) = fixture(FakeLibrary::initialized());
                let response = call(&ports, input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 4, "all golden cases executed");
}
