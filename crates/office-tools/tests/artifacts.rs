//! Artifact tools through their public dispatch boundary and fake ports.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_core::ports::Ports;
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
    let ports = ports(orbit);
    // The golden format deliberately uses JSON flow values (valid YAML), so
    // this narrow fixture reader needs no YAML or networking dependency.
    for verb in ["artifact-promote", "artifact-show"] {
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
