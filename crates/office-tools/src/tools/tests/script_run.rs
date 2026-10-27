use office_testkit::{envelope, fakes::PortsBuilder};
use serde_json::{Value, json};

#[test]
fn unapproved_script_is_refused_through_dispatch() {
    let request = office_core::protocol::parse(&envelope::request(
        "office.script-run",
        json!({"artifact":"art-19700101-missing","sha256":"0".repeat(64),
            "inputs":[],"args":[],"output":{"title":"Result","kind":"data","series":"result"}}),
        json!({}),
    ))
    .unwrap();
    let response: Value =
        serde_json::from_str(&crate::serve(&request, &PortsBuilder::default().build())).unwrap();
    assert_eq!(
        response["error"]["code"], "script_not_approved",
        "{response}"
    );
}

fn input() -> Value {
    json!({"artifact":"art-19700101-script","sha256":"0".repeat(64),"inputs":[],"args":[],"output":{"title":"Result","kind":"data","series":"result"}})
}
fn fixture(
    kind: office_core::domain::artifact::ArtifactKind,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeOrbit,
    office_testkit::fakes::FakeScripts,
) {
    use office_core::domain::artifact::*;
    use office_core::ports::Library;
    use office_testkit::fakes::*;
    let library = FakeLibrary::initialized();
    let artifact = Artifact::approved(
        ArtifactManifest {
            event_key: None,
            kind,
            title: "Script".into(),
            summary: String::new(),
            files: vec!["script.py".into()],
            cites: vec![],
            series: "script".into(),
            tags: vec![],
            runbook: None,
            sensitivity: Default::default(),
        },
        "fixture".into(),
        Producer {
            crew: None,
            runbook: None,
            at: None,
        },
        Approval {
            by: Approver::User,
            at: "1970-01-01T00:00:00Z".into(),
            edited: false,
        },
    )
    .unwrap();
    library
        .write_document(
            std::path::Path::new(&artifact.path().unwrap()),
            &artifact.to_markdown().unwrap(),
        )
        .unwrap();
    let orbit = FakeOrbit::default();
    let scripts = FakeScripts::default();
    let ports = office_core::ports::Ports {
        library: Box::new(library),
        orbit: Box::new(orbit.clone()),
        scripts: Box::new(scripts.clone()),
        models: Box::new(FakeModels::new()),
        clock: Box::new(FakeClock(std::time::UNIX_EPOCH)),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    };
    (ports, orbit, scripts)
}
fn accept(orbit: &office_testkit::fakes::FakeOrbit, input: Value) {
    orbit.push_response(Ok(json!({"tasks":[{"id":"proposal","status":"done","tags":["proposal","proposal:routine"]}],"truncated":false})));
    orbit.push_response(Ok(json!({"content":json!({"kind":"routine","definition":{"steps":[{"kind":"plugin.tool_call","tool":"office.script-run","input":input}]}}).to_string()})));
}
fn call(ports: &office_core::ports::Ports<'_>, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(
        "office.script-run",
        input,
        json!({"run_id":"run-fixture"}),
    ))
    .unwrap();
    envelope::response(&crate::serve(&request, ports)).unwrap()
}
#[test]
fn kind_and_accepted_proposal_pin_are_required_before_runner_is_called() {
    use office_core::domain::artifact::ArtifactKind;
    let (ports, orbit, scripts) = fixture(ArtifactKind::Data);
    assert_eq!(
        call(&ports, input())["error"]["code"],
        "script_not_approved"
    );
    assert!(orbit.calls().is_empty());
    assert!(scripts.executions.borrow().is_empty());
    for state in [
        json!({"tasks":[],"truncated":false}),
        json!({"tasks":[],"truncated":true}),
    ] {
        let (ports, orbit, scripts) = fixture(ArtifactKind::Script);
        orbit.push_response(Ok(state));
        assert_eq!(
            call(&ports, input())["error"]["code"],
            "script_not_approved"
        );
        assert!(scripts.executions.borrow().is_empty());
    }
    let (ports, orbit, scripts) = fixture(ArtifactKind::Script);
    let mut changed = input();
    changed["sha256"] = json!("1".repeat(64));
    accept(&orbit, changed);
    assert_eq!(
        call(&ports, input())["error"]["code"],
        "script_not_approved"
    );
    assert!(scripts.executions.borrow().is_empty());
}
#[test]
fn outputs_file_exactly_one_task_and_empty_runs_file_nothing() {
    use office_core::domain::{
        artifact::ArtifactKind,
        script::{ScriptFile, ScriptResult},
    };
    for has_output in [false, true] {
        let (ports, orbit, scripts) = fixture(ArtifactKind::Script);
        accept(&orbit, input());
        let files = if has_output {
            ["report.csv", "artifact-manifest.json", "script-run.json"]
                .iter()
                .map(|name| ScriptFile {
                    path: (*name).into(),
                    source_path: ports
                        .library
                        .stage_attachment(name, "fixture output")
                        .unwrap(),
                })
                .collect()
        } else {
            vec![]
        };
        scripts.push_response(Ok((
            ScriptResult {
                exit_code: Some(0),
                files: if has_output {
                    vec!["report.csv".into()]
                } else {
                    vec![]
                },
                ..Default::default()
            },
            files,
        )));
        if has_output {
            orbit.push_response(Ok(json!({"id":"output-task"})));
            for _ in 0..3 {
                orbit.push_response(Ok(json!({})));
            }
        }
        let result = call(&ports, input());
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(
            orbit
                .calls()
                .iter()
                .filter(|c| c.tool == "orbit.task.add")
                .count(),
            usize::from(has_output)
        );
        let puts: Vec<_> = orbit
            .calls()
            .into_iter()
            .filter(|c| c.tool == "orbit.task.artifact.put")
            .collect();
        assert_eq!(puts.len(), if has_output { 3 } else { 0 });
        if has_output {
            let calls = orbit.calls();
            let add = calls.iter().find(|c| c.tool == "orbit.task.add").unwrap();
            assert_eq!(add.input["tags"], json!(["script-output"]));
            assert!(!add.input.contains_key("status"));
            assert!(puts.iter().all(|c| c.input["id"] == "output-task"));
        }
    }
}
#[test]
fn preparation_uses_public_entry_before_proposal_is_accepted() {
    let (ports, orbit, scripts) = fixture(office_core::domain::artifact::ArtifactKind::Script);
    let request = serde_json::from_value(input()).unwrap();
    crate::prepare_script(&ports, &request).unwrap();
    assert_eq!(scripts.preparations.borrow().len(), 1);
    assert!(orbit.calls().is_empty());
}
#[test]
fn request_is_closed_including_nested_output_and_identity() {
    for (key, value) in [
        ("run_id", json!("caller")),
        ("secret", json!("token")),
        ("args", json!(false)),
    ] {
        let (ports, orbit, scripts) = fixture(office_core::domain::artifact::ArtifactKind::Script);
        let mut request = input();
        request[key] = value;
        assert_eq!(call(&ports, request)["error"]["code"], "invalid_argument");
        assert!(orbit.calls().is_empty());
        assert!(scripts.executions.borrow().is_empty());
    }
    let (ports, _, _) = fixture(office_core::domain::artifact::ArtifactKind::Script);
    let mut request = input();
    request["output"]["extra"] = json!(true);
    assert_eq!(call(&ports, request)["error"]["code"], "invalid_argument");
}

#[test]
fn conformance_golden_replays_success_and_refusal_offline() {
    let (ports, orbit, scripts) = fixture(office_core::domain::artifact::ArtifactKind::Script);
    accept(&orbit, input());
    scripts.push_response(Ok((
        office_core::domain::script::ScriptResult {
            exit_code: Some(0),
            duration_ms: 12,
            log_tail: "done".into(),
            run_dir: "fixture-run".into(),
            ..Default::default()
        },
        vec![],
    )));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/script-run.yaml");
    let golden = std::fs::read_to_string(path).unwrap();
    let mut input = None;
    let mut cases = 0;
    for line in golden.lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, ok) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let response = call(&ports, input.take().unwrap());
                assert_eq!(response["ok"], ok, "{response}");
                if ok {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 2);
}

#[test]
fn changed_proposal_artifact_cannot_extend_a_digest_bound_approval() {
    for matches in [false, true] {
        let (ports, orbit, scripts) = fixture(office_core::domain::artifact::ArtifactKind::Script);
        let payload=json!({"kind":"auto_task","definition":{"steps":[{"kind":"plugin.tool_call","tool":"office.script-run","input":input()}]}}).to_string();
        let digest = if matches {
            office_core::domain::source::sha256_hex(payload.as_bytes())
        } else {
            "0".repeat(64)
        };
        orbit.push_response(Ok(json!({"tasks":[{"id":"proposal","status":"done","tags":["proposal","proposal:auto_task"],"execution_summary":format!("Office proposal payload sha256: {digest}\nUser accepted")}],"truncated":false})));
        orbit.push_response(Ok(json!({"content":payload})));
        scripts.push_response(Ok((
            office_core::domain::script::ScriptResult {
                exit_code: Some(0),
                ..Default::default()
            },
            vec![],
        )));
        let response = call(&ports, input());
        assert_eq!(response["ok"], matches, "{response}");
        if !matches {
            assert_eq!(response["error"]["code"], "script_not_approved");
        }
        assert_eq!(scripts.executions.borrow().len(), usize::from(matches));
    }
}
