//! Recall through public exec dispatch with recording ports and fixture documents.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::path::Path;

use office_core::ports::{Library, Ports, PrError, PrState, PrView};
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels, FakeOrbit, FakePullRequests},
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
fn fixture(library: FakeLibrary) -> (Ports<'static>, FakeOrbit, FakeModels, FakePullRequests) {
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    let orbit = FakeOrbit::default();
    let models = FakeModels::new();
    let pr = FakePullRequests::default();
    ports.library = Box::new(library);
    ports.orbit = Box::new(orbit.clone());
    ports.models = Box::new(models.clone());
    ports.pr = Box::new(pr.clone());
    (ports, orbit, models, pr)
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
    let (ports, orbit, models, _pr) = fixture(library);
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
            .any(|r| r["id"] == "acme/office#12" && r["reason"] == "pr_source_unavailable")
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
    let (ports, orbit, _, _pr) = fixture(library);
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
        json!({"question":" "}),
        json!({"question":"a","limit":0}),
        json!({"question":"a","limit":51}),
        json!({"limit":0}),
        json!({"limit":201}),
        json!({"session_id":""}),
        json!({"session_id":"two\nlines"}),
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
    let (ports, orbit, _, _pr) = fixture(library);
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
fn recall_pr_refs_report_merged_and_name_unavailable_or_failed_causes() {
    let library = FakeLibrary::initialized();
    source(
        &library,
        "src-a",
        "Topic",
        "work",
        false,
        "[2026-10-28T10:00:00Z] action: Ship it.\n",
        json!({"L1":[{"kind":"pr","id":"acme/office#12"}]}),
    );
    let (ports, _orbit, _models, pr) = fixture(library);
    pr.push_response(Ok(PrView {
        state: PrState::Merged,
        merged_at: Some("2026-10-28T11:00:00Z".into()),
        merge_commit: Some("0123456789abcdef".into()),
    }));
    let response = call(&ports, json!({"question":"topic"}));
    assert_eq!(response["ok"], true, "{response}");
    let merged = &response["output"]["refs"][0];
    assert_eq!(merged["kind"], "pr");
    assert_eq!(merged["id"], "acme/office#12");
    assert_eq!(merged["status"], "merged");
    assert_eq!(merged["merged_at"], "2026-10-28T11:00:00Z");
    assert_eq!(merged["merge_commit"], "0123456789abcdef");
    assert_eq!(pr.calls(), vec!["acme/office#12".to_string()]);

    for (reply, reason) in [
        (Err(PrError::Unavailable), "pr_source_unavailable"),
        (Err(PrError::Failed), "pr_lookup_failed"),
        (Err(PrError::InvalidResponse), "invalid_pr_response"),
        (Err(PrError::InvalidRef), "invalid_pr_ref"),
    ] {
        pr.push_response(reply);
        let response = call(&ports, json!({"question":"topic"}));
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["output"]["refs"][0]["status"], "unresolved");
        assert_eq!(response["output"]["refs"][0]["reason"], reason);
        assert!(!response.to_string().contains("gh secret diagnostics"));
    }
}

#[test]
fn recall_by_session_id_lists_only_that_session_newest_first() {
    let library = FakeLibrary::initialized();
    library.write_document(Path::new("sources/conversation/2026/10/src-mine/source.md"), "---\nid: src-mine\nkind: conversation\ntitle: Mine\nproject: /work/a\nsensitivity: normal\ncaptured_at: \"2026-10-28T00:00:00Z\"\nsession_id: sess-mine-1\nentry_refs: {}\n---\n[2026-10-28T10:00:00Z] decision: First captured.\n[2026-10-28T11:00:00Z] fact: Second captured.\n").unwrap();
    source(
        &library,
        "src-other",
        "Other",
        "/work/a",
        false,
        "[2026-10-28T12:00:00Z] decision: Someone else.\n",
        json!({}),
    );
    let (ports, _, _, _pr) = fixture(library);
    let response = call(&ports, json!({"question":"sess-mine-1"}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    let texts: Vec<&str> = out["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["Second captured.", "First captured."]);
    assert_eq!(out["sessions"][0]["session_id"], "sess-mine-1");
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
                let (ports, _, _, _pr) = fixture(FakeLibrary::initialized());
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
    assert_eq!(count, 5, "all golden cases executed");
}

/// Every object in `items` has the schema's required keys and no others.
fn assert_closed_shape(items: &Value, schema: &Value) {
    let properties = schema["properties"].as_object().unwrap();
    for item in items.as_array().unwrap() {
        let item = item.as_object().unwrap();
        for key in schema["required"].as_array().unwrap() {
            assert!(
                item.contains_key(key.as_str().unwrap()),
                "{key} in {item:?}"
            );
        }
        for key in item.keys() {
            assert!(properties.contains_key(key), "schema lists {key}");
        }
    }
}

fn session_source(
    library: &FakeLibrary,
    id: &str,
    session: Option<&str>,
    private: bool,
    body: &str,
) {
    let session = session.map_or(String::new(), |s| format!("session_id: {s}\n"));
    library
        .write_document(
            Path::new(&format!("sources/conversation/2026/10/{id}/source.md")),
            &format!(
                "---\nid: {id}\nkind: conversation\ntitle: Title {id}\nproject: /work/a\nsensitivity: {}\ncaptured_at: \"2026-10-28T00:00:00Z\"\n{session}entry_refs: {{}}\n---\n{body}",
                if private { "private" } else { "normal" }
            ),
        )
        .unwrap();
}

#[test]
fn feed_mode_without_question_lists_newest_first_scoped_to_one_session_up_to_200() {
    let library = FakeLibrary::initialized();
    // 210 entries in the window's session, spread over minutes so order is total.
    let body: String = (0..210)
        .map(|n| {
            format!(
                "[2026-10-28T{:02}:{:02}:00Z] fact: Entry {n}.\n",
                8 + n / 60,
                n % 60
            )
        })
        .collect();
    session_source(&library, "src-mine", Some("session-mine"), false, &body);
    session_source(
        &library,
        "src-other",
        Some("session-other"),
        false,
        "[2026-10-29T00:00:00Z] decision: Newest elsewhere.\n",
    );
    session_source(
        &library,
        "src-legacy",
        None,
        false,
        "[2026-10-29T01:00:00Z] fact: No session identity.\n",
    );
    session_source(
        &library,
        "src-private",
        Some("session-private"),
        true,
        "[2026-10-29T02:00:00Z] fact: PRIVATE FEED TEXT\n",
    );
    let (ports, _, models, _pr) = fixture(library);

    // Whole-Library feed: newest first across sources, private text withheld.
    let response = call(&ports, json!({"limit":3}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    let texts: Vec<&str> = out["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        ["No session identity.", "Newest elsewhere."],
        "the private entry counts toward the limit but is withheld"
    );
    assert!(!response.to_string().contains("PRIVATE FEED TEXT"));
    let ids: Vec<&Value> = out["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| &s["session_id"])
        .collect();
    assert_eq!(
        ids,
        [
            &json!("session-private"),
            &json!(null),
            &json!("session-other")
        ]
    );

    // One session's feed, at the window's full limit of 200.
    let response = call(&ports, json!({"session_id":"session-mine","limit":200}));
    assert_eq!(response["ok"], true, "{response}");
    let out = &response["output"];
    let entries = out["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 200);
    assert_eq!(entries[0]["text"], "Entry 209.");
    assert_eq!(entries[199]["text"], "Entry 10.");
    assert!(entries.iter().all(|e| e["source_id"] == "src-mine"));
    let times: Vec<&str> = entries
        .iter()
        .map(|e| e["time"].as_str().unwrap())
        .collect();
    assert!(times.windows(2).all(|w| w[0] > w[1]), "newest first");
    assert_eq!(out["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(out["sessions"][0]["session_id"], "session-mine");

    // The session filter is an exact source identity, not a search term:
    // a prefix matches nothing, and it composes with a question.
    let response = call(&ports, json!({"session_id":"session-mi"}));
    assert_eq!(response["output"]["entries"], json!([]));
    let response = call(
        &ports,
        json!({"question":"elsewhere","session_id":"session-mine"}),
    );
    assert_eq!(response["output"]["entries"], json!([]));
    let response = call(
        &ports,
        json!({"question":"entry 209","session_id":"session-mine"}),
    );
    assert_eq!(response["output"]["entries"].as_array().unwrap().len(), 1);
    // Default feed limit is 20; a question still caps at 50.
    assert_eq!(
        call(&ports, json!({"session_id":"session-mine"}))["output"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    assert_eq!(
        call(&ports, json!({"question":"entry","limit":200}))["error"]["code"],
        "invalid_argument"
    );
    assert!(
        models.extract_requests.borrow().is_empty(),
        "recall makes no model call"
    );

    // The output matches the shipped response schema's closed item shapes.
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugin/.orbit-plugin/schemas/recall.response.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let response = call(&ports, json!({"limit":200}));
    assert_closed_shape(
        &response["output"]["sessions"],
        &schema["properties"]["sessions"]["items"],
    );
    assert_closed_shape(
        &response["output"]["entries"],
        &schema["properties"]["entries"]["items"],
    );
    assert!(
        schema["properties"]["sessions"]["items"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("session_id"))
    );
}
