//! Artifact tools through their public dispatch boundary and fake ports.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_core::ports::{Library, Ports};
use office_testkit::{
    envelope,
    fakes::{FakeClock, FakeLibrary, FakeModels, FakeOrbit},
};
use serde_json::{Value, json};

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn ports(orbit: FakeOrbit) -> Ports<'static> {
    Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(FakeLibrary::initialized()),
        models: Box::new(FakeModels::new()),
        orbit: Box::new(orbit),
        clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

fn seed(orbit: &FakeOrbit, binary: bool) {
    orbit.push_response(Ok(
        json!({"id":"fixture-task","status":"review","crew":"fixture-crew",
        "updated_at":"1970-01-01T00:00:00Z", "artifacts":[
            {"path":"artifact-manifest.json"}, {"path":"report.md"}]}),
    ));
    orbit.push_response(Ok(json!({"content":json!({"kind":"report","title":"Fixture report",
        "summary":"A fixture", "files":["report.md"], "series":"fixture-report", "cites":[], "tags":[]}).to_string()})));
    orbit.push_response(Ok(if binary {
        json!({"content_base64":"AP+A"})
    } else {
        json!({"content":"Approved report\n"})
    }));
}

#[test]
fn promotes_versions_and_shows_provenance_without_deleting_the_previous_output() {
    let orbit = FakeOrbit::default();
    seed(&orbit, false);
    seed(&orbit, true);
    let ports = ports(orbit.clone());
    let first = call(
        &ports,
        "office.artifact-promote",
        json!({"task":"fixture-task","edited":true}),
    );
    assert_eq!(first["ok"], true, "{first}");
    let id = first["output"]["id"].as_str().unwrap();
    let show = call(&ports, "office.artifact-show", json!({"id":id}));
    assert_eq!(show["ok"], true, "{show}");
    assert_eq!(show["output"]["artifact"]["approved"]["by"], "user");
    assert_eq!(show["output"]["artifact"]["approved"]["edited"], true);
    assert_eq!(
        show["output"]["artifact"]["produced_by"]["crew"],
        "fixture-crew"
    );
    let second = call(
        &ports,
        "office.artifact-promote",
        json!({"task":"fixture-task"}),
    );
    assert_eq!(second["ok"], true, "{second}");
    assert_eq!(second["output"]["version"], 2);
    assert_eq!(second["output"]["supersedes"], id);
    assert_ne!(second["output"]["id"], id);
    let show = call(&ports, "office.artifact-show", json!({"id":id}));
    assert_eq!(
        show["output"]["series_history"].as_array().unwrap().len(),
        2
    );
    assert_eq!(show["output"]["artifact"]["version"], 1);
    assert_eq!(orbit.calls().len(), 6);
}

fn seed_custom(orbit: &FakeOrbit, title: &str, series: &str, payload: Value) {
    orbit.push_response(Ok(
        json!({"id":"fixture-task","status":"review","crew":"fixture-crew",
        "updated_at":"1970-01-01T00:00:00Z", "artifacts":[
            {"path":"artifact-manifest.json"}, {"path":"report.md"}]}),
    ));
    orbit.push_response(Ok(json!({"content":json!({"kind":"report","title":title,
        "summary":"A fixture", "files":["report.md"], "series":series, "cites":[], "tags":[]}).to_string()})));
    orbit.push_response(Ok(payload));
}

fn promote_and_show(orbit: &FakeOrbit) -> Value {
    let ports = ports(orbit.clone());
    let promoted = call(
        &ports,
        "office.artifact-promote",
        json!({"task":"fixture-task"}),
    );
    assert_eq!(promoted["ok"], true, "{promoted}");
    let id = promoted["output"]["id"].as_str().unwrap();
    let show = call(&ports, "office.artifact-show", json!({"id":id}));
    assert_eq!(show["ok"], true, "{show}");
    show["output"].clone()
}

#[test]
fn artifact_show_returns_the_text_body_of_a_report() {
    let orbit = FakeOrbit::default();
    seed_custom(
        &orbit,
        "Body report",
        "body-report",
        json!({"content":"Line one [src-19700101-x#L1]\nLine two\n"}),
    );
    let output = promote_and_show(&orbit);
    assert_eq!(output["body"], "Line one [src-19700101-x#L1]\nLine two\n");
    assert_eq!(output["truncated"], false);
}

#[test]
fn artifact_show_truncates_a_body_past_the_byte_limit() {
    let orbit = FakeOrbit::default();
    seed_custom(
        &orbit,
        "Long report",
        "long-report",
        json!({"content":"x".repeat(20_000)}),
    );
    let output = promote_and_show(&orbit);
    assert_eq!(output["truncated"], true);
    assert_eq!(output["body"].as_str().unwrap().len(), 16_384);
}

#[test]
fn artifact_show_returns_no_body_for_a_binary_artifact() {
    let orbit = FakeOrbit::default();
    seed_custom(
        &orbit,
        "Binary report",
        "binary-report",
        json!({"content_base64":"AP+A"}),
    );
    let output = promote_and_show(&orbit);
    assert!(output.get("body").is_none(), "{output}");
    assert_eq!(output["truncated"], false);
}

#[test]
fn refuses_tasks_without_artifacts_with_a_distinct_code() {
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(
        json!({"id":"fixture-task","status":"review","artifacts":[]}),
    ));
    let response = call(
        &ports(orbit.clone()),
        "office.artifact-promote",
        json!({"task":"fixture-task"}),
    );
    assert_eq!(
        response["error"]["code"], "task_has_no_artifacts",
        "{response}"
    );
    assert_eq!(orbit.calls().len(), 1);
}

#[test]
fn conformance_goldens_replay_with_offline_task_artifacts_and_a_fixed_clock() {
    let orbit = FakeOrbit::default();
    seed(&orbit, false);
    let ports = ports(orbit.clone());
    // The golden format deliberately uses JSON flow values (valid YAML), so
    // this narrow fixture reader needs no YAML or networking dependency.
    for verb in ["artifact-promote", "artifact-show"] {
        if verb == "artifact-show" {
            // Seed the truncated and binary artifacts the golden replays,
            // through the same public promote path as its first case.
            seed_custom(
                &orbit,
                "Long report",
                "long-report",
                json!({"content":"x".repeat(20_000)}),
            );
            seed_custom(
                &orbit,
                "Binary report",
                "binary-report",
                json!({"content_base64":"AP+A"}),
            );
            for _ in 0..2 {
                let promoted = call(
                    &ports,
                    "office.artifact-promote",
                    json!({"task":"fixture-task"}),
                );
                assert_eq!(promoted["ok"], true, "{promoted}");
            }
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(path).unwrap();
        let mut input = None;
        let mut cases = 0;
        for line in golden.lines() {
            if let Some(value) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<Value>(value).unwrap());
            }
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(value) = line.strip_prefix(prefix) {
                    let expect: Value = serde_json::from_str(value).unwrap();
                    let response = call(&ports, &format!("office.{verb}"), input.take().unwrap());
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        assert_eq!(response["output"], expect);
                    } else {
                        assert_eq!(response["error"]["code"], expect["code"]);
                    }
                    cases += 1;
                }
            }
        }
        assert!(
            cases >= 2,
            "the fixture runner must execute success and refusal cases"
        );
    }
}

#[test]
fn refusals_never_publish_partial_outputs_or_accept_caller_approval_provenance() {
    for input in [
        json!({"task":"fixture-task","edited":"true"}),
        json!({"task":"fixture-task","approved":{"by":"routine"}}),
    ] {
        let orbit = FakeOrbit::default();
        let ports = ports(orbit.clone());
        let response = call(&ports, "office.artifact-promote", input);
        assert_eq!(response["error"]["code"], "invalid_argument");
        assert!(orbit.calls().is_empty());
        assert!(ports.library.artifact_documents().unwrap().is_empty());
    }
    for status in ["in-progress", "backlog", "proposed"] {
        let orbit = FakeOrbit::default();
        orbit.push_response(Ok(json!({"id":"fixture-task","status":status,"artifacts":[{"path":"artifact-manifest.json"}]})));
        let ports = ports(orbit);
        let response = call(
            &ports,
            "office.artifact-promote",
            json!({"task":"fixture-task"}),
        );
        assert_eq!(response["error"]["code"], "task_not_reviewable");
        assert!(ports.library.artifact_documents().unwrap().is_empty());
    }
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"id":"fixture-task","status":"review","artifacts":[{"path":"artifact-manifest.json"},{"path":"first.txt"},{"path":"second.bin"}]})));
    orbit.push_response(Ok(json!({"content":json!({"kind":"data","title":"Two files","files":["first.txt","second.bin"],"series":"two-files"}).to_string()})));
    orbit.push_response(Ok(json!({"content":"read successfully"})));
    orbit.push_response(Err(office_core::Error::OrbitUnavailable));
    let ports = ports(orbit);
    let response = call(
        &ports,
        "office.artifact-promote",
        json!({"task":"fixture-task"}),
    );
    assert_eq!(response["error"]["code"], "orbit_unavailable");
    assert!(ports.library.artifact_documents().unwrap().is_empty());
}

fn list_ports(library: FakeLibrary) -> Ports<'static> {
    Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(FakeModels::new()),
        orbit: Box::new(FakeOrbit::default()),
        clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

fn seed_list_fixtures(library: &FakeLibrary) {
    for (id, kind, series, title, approved, sensitivity) in [
        (
            "art-20261001-alpha-report",
            "report",
            "alpha",
            "Alpha report",
            "2026-10-01T10:00:00Z",
            "normal",
        ),
        (
            "art-20261003-beta-report",
            "report",
            "beta",
            "Beta report",
            "2026-10-03T10:00:00Z",
            "private",
        ),
        (
            "art-20261002-alpha-data",
            "data",
            "alpha",
            "Alpha data",
            "2026-10-02T10:00:00Z",
            "normal",
        ),
    ] {
        let path = format!("artifacts/{}/{}/{}/artifact.md", &id[4..8], &id[8..10], id);
        library
            .write_document(
                std::path::Path::new(&path),
                &format!("---\nid: {id}\nkind: {kind}\ntitle: {title}\nsummary: Fixture\nfiles: [report.md]\ntask: fixture-task\nproduced_by: {{ crew: fixture-crew, at: \"2026-09-01T00:00:00Z\" }}\napproved: {{ by: user, at: \"{approved}\", edited: false }}\ncites: []\nseries: {series}\nversion: 1\nsensitivity: {sensitivity}\ntags: []\n---\n"),
            )
            .unwrap();
    }
}

#[test]
fn artifact_list_filters_sorts_newest_first_and_bounds_with_total_and_truncated() {
    let empty = call(
        &list_ports(FakeLibrary::initialized()),
        "office.artifact-list",
        json!({}),
    );
    assert_eq!(
        empty["output"],
        json!({"artifacts":[],"total":0,"truncated":false}),
        "{empty}"
    );
    let library = FakeLibrary::initialized();
    seed_list_fixtures(&library);
    let ports = list_ports(library);
    let all = call(&ports, "office.artifact-list", json!({}));
    assert_eq!(all["ok"], true, "{all}");
    let ids: Vec<_> = all["output"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "art-20261003-beta-report",
            "art-20261002-alpha-data",
            "art-20261001-alpha-report"
        ]
    );
    assert_eq!(all["output"]["total"], 3);
    assert_eq!(all["output"]["truncated"], false);
    assert_eq!(
        all["output"]["artifacts"][0],
        json!({"id":"art-20261003-beta-report","kind":"report","series":"beta","title":"Beta report","approved_at":"2026-10-03T10:00:00Z","sensitivity":"private"})
    );
    let by_kind = call(&ports, "office.artifact-list", json!({"kind":"report"}));
    let ids: Vec<_> = by_kind["output"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["art-20261003-beta-report", "art-20261001-alpha-report"]
    );
    assert_eq!(by_kind["output"]["total"], 2);
    let by_series = call(&ports, "office.artifact-list", json!({"series":"alpha"}));
    let ids: Vec<_> = by_series["output"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["art-20261002-alpha-data", "art-20261001-alpha-report"]
    );
    assert_eq!(by_series["output"]["total"], 2);
    let bounded = call(&ports, "office.artifact-list", json!({"limit":2}));
    assert_eq!(bounded["output"]["artifacts"].as_array().unwrap().len(), 2);
    assert_eq!(bounded["output"]["total"], 3);
    assert_eq!(bounded["output"]["truncated"], true);
    let exact = call(&ports, "office.artifact-list", json!({"limit":3}));
    assert_eq!(exact["output"]["truncated"], false);
}

#[test]
fn artifact_list_refuses_invalid_filters_limits_and_unknown_keys() {
    let ports = list_ports(FakeLibrary::initialized());
    for input in [
        json!({"kind":"bogus"}),
        json!({"kind":42}),
        json!({"series":""}),
        json!({"series":42}),
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"limit":"2"}),
        json!({"extra":true}),
    ] {
        let response = call(&ports, "office.artifact-list", input);
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
}

#[test]
fn artifact_list_conformance_golden_replays_through_public_dispatch() {
    let library = FakeLibrary::initialized();
    seed_list_fixtures(&library);
    let ports = list_ports(library);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/artifact-list.yaml");
    let golden = std::fs::read_to_string(path).unwrap();
    let mut input = None;
    let mut cases = 0;
    for line in golden.lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expect: Value = serde_json::from_str(value).unwrap();
                let response = call(&ports, "office.artifact-list", input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expect);
                } else {
                    assert_eq!(response["error"]["code"], expect["code"]);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 5, "all artifact-list golden cases must run");
}

#[test]
fn proposed_script_output_can_be_explicitly_promoted_without_executor_pickup() {
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"id":"output-task","status":"proposed","tags":["script-output"],"artifacts":[{"path":"artifact-manifest.json"},{"path":"report.md"}]})));
    orbit.push_response(Ok(json!({"content":json!({"kind":"data","title":"Script output","files":["report.md"],"series":"script-output"}).to_string()})));
    orbit.push_response(Ok(json!({"content":"output"})));
    let response = call(
        &ports(orbit),
        "office.artifact-promote",
        json!({"task":"output-task"}),
    );
    assert_eq!(response["ok"], true, "{response}");
}
