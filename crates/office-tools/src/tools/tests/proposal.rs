use office_core::ports::Ports;
use office_testkit::{envelope, fakes::*};
use serde_json::{Value, json};

fn fixture() -> (Ports<'static>, FakeOrbit) {
    let orbit = FakeOrbit::default();
    let library = FakeLibrary::initialized();
    let ports = Ports {
        library: Box::new(library),
        orbit: Box::new(orbit.clone()),
        scripts: Box::new(FakeScripts::default()),
        models: Box::new(FakeModels::new()),
        clock: Box::new(FakeClock(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(86400 * 40),
        )),
        selections: Box::new(FakeSelections::default()),
    };
    (ports, orbit)
}

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&crate::serve(&request, ports)).unwrap()
}

fn auto_task() -> Value {
    json!({"kind":"auto_task","definition":{"name":"weekly-report","schedule":{"cron":"0 9 * * 1"},"template":{"title":"Write the report","status":"backlog"}}})
}

fn create_input() -> Value {
    json!({"title":"Weekly report","description":"Repeated work","evidence":["task-a","task-b"],"payload":auto_task(),"tags":["onboarding"]})
}

#[test]
fn proposal_create_files_proposed_task_payload_and_evidence() {
    let (ports, orbit) = fixture();
    orbit.push_response(Ok(json!({"tasks":[],"truncated":false})));
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed"})));
    orbit.push_response(Ok(json!({})));
    let response = call(&ports, "office.proposal-create", create_input());
    assert_eq!(response["ok"], true, "{response}");
    let calls = orbit.calls();
    assert_eq!(calls[1].tool, "orbit.task.add");
    assert!(
        !calls[1].input.contains_key("status"),
        "Orbit task.add always creates proposed tasks and accepts no status override"
    );
    assert_eq!(calls[1].input["complexity"], "low");
    assert!(
        calls[1].input["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("onboarding"))
    );
    assert_eq!(calls[2].input["path"], "proposal.json");
    assert_eq!(
        response["output"]["evidence_page"],
        "proposals/proposal-fixture.md"
    );
}

#[test]
fn proposal_accept_creates_and_enables_once_then_refuses_closed() {
    let (ports, orbit) = fixture();
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed","tags":["proposal","proposal:auto_task"]})));
    orbit.push_response(Ok(json!({"content":auto_task().to_string()})));
    // Reserve the proposal through Orbit's atomic start transition.
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"in-progress"})));
    orbit.push_response(Ok(json!({"name":"weekly-report","enabled":false})));
    orbit.push_response(Ok(json!({"name":"weekly-report","enabled":true})));
    orbit.push_response(Ok(json!({"status":"review"})));
    orbit.push_response(Ok(json!({"status":"done"})));
    let response = call(&ports, "office.proposal-accept", approval(&auto_task()));
    assert_eq!(response["ok"], true, "{response}");
    orbit.push_response(Ok(
        json!({"id":"proposal-fixture","status":"done","tags":["proposal"]}),
    ));
    let closed = call(&ports, "office.proposal-accept", approval(&auto_task()));
    assert_eq!(closed["error"]["code"], "proposal_closed", "{closed}");
    let calls = orbit.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.tool == "orbit.auto_task.add")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.tool == "orbit.auto_task.update")
            .count(),
        1
    );
    assert_eq!(
        calls[4].input,
        json!({"name":"weekly-report","enabled":true})
            .as_object()
            .unwrap()
            .clone()
    );
}

#[test]
fn proposal_create_refuses_routine_payload_without_callbacks() {
    let (ports, orbit) = fixture();
    let mut input = create_input();
    input["payload"]["kind"] = json!("routine");
    let response = call(&ports, "office.proposal-create", input);
    assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    assert!(orbit.calls().is_empty());
}

#[test]
fn proposal_dismissal_suppresses_equivalent_evidence_but_expires() {
    for (date, suppressed) in [
        ("1970-01-12T00:00:00Z", true),
        ("1970-01-11T00:00:00Z", false),
    ] {
        let (ports, orbit) = fixture();
        let tag = office_core::domain::proposal::cluster(
            office_core::domain::proposal::ProposalKind::AutoTask,
            &["task-b".into(), "task-a".into()],
        )
        .unwrap();
        orbit.push_response(Ok(json!({"tasks":[{"id":"dismissed","status":"rejected","tags":[tag],"updated_at":date}],"truncated":false})));
        if !suppressed {
            orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed"})));
            orbit.push_response(Ok(json!({})));
        }
        let response = call(&ports, "office.proposal-create", create_input());
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["output"]["suppressed"], suppressed);
        assert_eq!(orbit.calls().len(), if suppressed { 1 } else { 3 });
        if suppressed {
            assert_eq!(ports.library.head_commit().unwrap(), None);
        }
    }
    let (ports, orbit) = fixture();
    orbit.push_response(Ok(json!({"tasks":[],"truncated":true})));
    assert_eq!(
        call(&ports, "office.proposal-create", create_input())["error"]["code"],
        "proposal_history_incomplete"
    );
    assert_eq!(orbit.calls().len(), 1);
}

fn accept_replies(orbit: &FakeOrbit, payload: &Value) {
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed","tags":["proposal",format!("proposal:{}",payload["kind"].as_str().unwrap())]})));
    orbit.push_response(Ok(json!({"content":payload.to_string()})));
}
fn reserve(orbit: &FakeOrbit) {
    orbit.push_response(Ok(json!({"status":"in-progress"})));
}
fn close(orbit: &FakeOrbit) {
    orbit.push_response(Ok(json!({"status":"review"})));
    orbit.push_response(Ok(json!({"status":"done"})));
}

#[test]
fn proposal_retire_disables_only_the_named_auto_task() {
    let (ports, orbit) = fixture();
    let payload = json!({"kind":"retire","target":{"type":"auto_task","name":"weekly-report"}});
    accept_replies(&orbit, &payload);
    reserve(&orbit);
    orbit.push_response(Ok(json!({"name":"weekly-report","enabled":false})));
    close(&orbit);
    let response = call(&ports, "office.proposal-accept", approval(&payload));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        orbit.calls()[3].input,
        json!({"name":"weekly-report","enabled":false})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(
        orbit
            .calls()
            .iter()
            .all(|c| c.tool != "orbit.auto_task.add")
    );
}

#[test]
fn proposal_knowledge_add_materializes_the_template_and_cites_evidence() {
    use office_core::ports::Library;
    use std::path::Path;
    let (mut ports, orbit) = fixture();
    ports.library = Box::new(FakeLibrary::with_bundled_templates());
    ports
        .library
        .write_document(
            Path::new("sources/src-fixture/source.md"),
            "---\nid: src-fixture\nkind: text\ntitle: Evidence\n---\nEvidence text\n",
        )
        .unwrap();
    let payload = json!({"kind":"knowledge_add","page":{"id":"dom-topic-pricing","title":"Pricing","template":"topic","sections":{"Overview":"Prices use annual contracts."}}});
    let staging = {
        // A fresh fake keeps its staging observer when moved into ports.
        let library = FakeLibrary::with_bundled_templates();
        library
            .write_document(
                Path::new("sources/src-fixture/source.md"),
                "---\nid: src-fixture\nkind: text\ntitle: Evidence\n---\nEvidence text\n",
            )
            .unwrap();
        let observer = library.staging();
        ports.library = Box::new(library);
        observer
    };
    orbit.push_response(Ok(json!({"tasks":[],"truncated":false})));
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed"})));
    orbit.push_response(Ok(json!({})));
    let input = json!({"title":"Pricing","description":"Add evidenced pricing knowledge","evidence":["src-fixture#L1"],"payload":payload});
    let created = call(&ports, "office.proposal-create", input);
    assert_eq!(created["ok"], true, "{created}");
    let path = orbit.calls()[2].input["source_path"]
        .as_str()
        .unwrap()
        .to_string();
    let filed: Value = serde_json::from_str(&staging.text(&path).unwrap()).unwrap();
    let markdown = filed["markdown"].as_str().unwrap();
    assert!(markdown.starts_with("---\nid: \"dom-topic-pricing\""));
    assert!(markdown.contains("## Overview\nPrices use annual contracts."));
    assert!(markdown.contains("## How it works here"));
    assert!(markdown.contains("## Key numbers"));
    assert!(markdown.contains("cites: [\"src-fixture#L1\"]"));
    assert!(ports.library.get("dom-topic-pricing").unwrap().is_none());
    assert!(
        ports
            .library
            .read_text(Path::new("proposals/proposal-fixture.md"))
            .unwrap()
            .contains("src-fixture")
    );
    assert!(staging.live().is_empty());
    orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed","tags":["proposal","proposal:knowledge_add"]})));
    orbit.push_response(Ok(json!({"content":staging.text(&path).unwrap()})));
    reserve(&orbit);
    close(&orbit);
    let accepted = call(
        &ports,
        "office.proposal-accept",
        json!({"id":"proposal-fixture","sha256":created["output"]["payload_sha256"]}),
    );
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert_eq!(
        ports
            .library
            .read_text(Path::new("knowledge/domain/dom-topic-pricing.md"))
            .unwrap(),
        markdown
    );
}

struct PreparingScripts {
    observed: FakeScripts,
    orbit: FakeOrbit,
    fail: bool,
}
impl office_core::ports::ScriptRunner for PreparingScripts {
    fn prepare(
        &self,
        script: &office_core::domain::script::ScriptPreparation,
    ) -> office_core::Result<()> {
        assert!(self.orbit.calls().iter().all(|c| {
            ![
                "orbit.auto_task.add",
                "orbit.auto_task.update",
                "orbit.task.update",
            ]
            .contains(&c.tool)
        }));
        self.observed.prepare(script)?;
        if self.fail {
            Err(office_core::domain::script::ScriptError::Prepare { exit_code: Some(1) }.into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn proposal_script_dependencies_are_prepared_before_any_automation_mutation() {
    use office_core::domain::artifact::*;
    use std::path::Path;
    for fail in [false, true] {
        let (mut ports, orbit) = fixture();
        let artifact = Artifact::approved(
            ArtifactManifest {
                kind: ArtifactKind::Script,
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
        ports
            .library
            .write_document(
                Path::new(&artifact.path().unwrap()),
                &artifact.to_markdown().unwrap(),
            )
            .unwrap();
        let scripts = FakeScripts::default();
        ports.scripts = Box::new(PreparingScripts {
            observed: scripts.clone(),
            orbit: orbit.clone(),
            fail,
        });
        let mut payload = auto_task();
        payload["definition"]["steps"] = json!([{"kind":"plugin.tool_call","tool":"office.script-run","input":{"artifact":"art-19700101-script","sha256":"0".repeat(64),"inputs":[],"args":[],"output":{"title":"Result","kind":"data","series":"result"}}}]);
        accept_replies(&orbit, &payload);
        if !fail {
            reserve(&orbit);
            orbit.push_response(Ok(json!({"name":"weekly-report","enabled":true})));
            orbit.push_response(Ok(json!({"name":"weekly-report","enabled":true})));
            close(&orbit);
        }
        let response = call(&ports, "office.proposal-accept", approval(&payload));
        assert_eq!(scripts.preparations.borrow().len(), 1);
        assert_eq!(response["ok"], !fail, "{response}");
        if fail {
            assert_eq!(response["error"]["code"], "script_prepare_failed");
            assert_eq!(orbit.calls().len(), 2);
        }
    }
}

#[test]
fn proposal_invalid_and_closed_requests_make_no_mutations() {
    for status in [
        "done",
        "rejected",
        "archived",
        "in-progress",
        "review",
        "backlog",
    ] {
        let (ports, orbit) = fixture();
        orbit.push_response(Ok(json!({"status":status,"tags":["proposal"]})));
        assert_eq!(
            call(&ports, "office.proposal-accept", approval(&auto_task()))["error"]["code"],
            "proposal_closed"
        );
        assert_eq!(orbit.calls().len(), 1);
        assert_eq!(ports.library.head_commit().unwrap(), None);
    }
    let (ports, orbit) = fixture();
    for payload in [
        json!({"id":"../../escape"}),
        json!({"id":"proposal-fixture","force":true}),
    ] {
        assert_eq!(
            call(&ports, "office.proposal-accept", payload)["error"]["code"],
            "invalid_argument"
        );
    }
    assert!(orbit.calls().is_empty());
}

#[test]
fn proposal_reservation_refusal_and_unknown_creation_outcome_never_enable_or_retry() {
    for fail_add in [false, true] {
        let (ports, orbit) = fixture();
        accept_replies(&orbit, &auto_task());
        if fail_add {
            reserve(&orbit);
            orbit.push_response(Err(office_core::Error::OrbitOutcomeUnknown));
        } else {
            orbit.push_response(Err(office_core::Error::OrbitRefused {
                code: "task_start_refused".into(),
            }));
        }
        assert_eq!(
            call(&ports, "office.proposal-accept", approval(&auto_task()))["ok"],
            false
        );
        assert!(
            orbit
                .calls()
                .iter()
                .all(|c| c.tool != "orbit.auto_task.update")
        );
        assert_eq!(
            orbit
                .calls()
                .iter()
                .filter(|c| c.tool == "orbit.auto_task.add")
                .count(),
            usize::from(fail_add)
        );
    }
}

#[test]
fn proposal_updates_retirements_and_runbooks_apply_reviewed_pages() {
    use office_core::domain::source::sha256_hex;
    use std::path::Path;
    for mode in ["update", "retire", "retire_crlf", "runbook", "stale"] {
        let (ports, orbit) = fixture();
        let text = "---\nid: dom-fixture\ntitle: Fixture\n---\nOriginal\n";
        let text = if mode == "retire_crlf" {
            text.replace("\n", "\r\n")
        } else {
            text.into()
        };
        let text = text.as_str();
        ports
            .library
            .write_document(Path::new("knowledge/domain/dom-fixture.md"), text)
            .unwrap();
        let payload = match mode {
            "update" => {
                json!({"kind":"knowledge_update","markdown":text.replace("Original","Reviewed"),"base_sha256":sha256_hex(text.as_bytes())})
            }
            "stale" => {
                json!({"kind":"knowledge_update","markdown":text.replace("Original","Reviewed"),"base_sha256":"0".repeat(64)})
            }
            "retire" | "retire_crlf" => {
                json!({"kind":"retire","target":{"type":"page","id":"dom-fixture","base_sha256":sha256_hex(text.as_bytes())}})
            }
            "runbook" => {
                json!({"kind":"runbook_from_friction","markdown":"---\nid: rb-fixture\ntitle: Fixture runbook\n---\nSteps based on the recurring friction.\n"})
            }
            _ => unreachable!(),
        };
        accept_replies(&orbit, &payload);
        if mode != "stale" {
            reserve(&orbit);
            close(&orbit);
        }
        let response = call(&ports, "office.proposal-accept", approval(&payload));
        if mode == "stale" {
            assert_eq!(response["error"]["code"], "proposal_revision_changed");
            assert_eq!(orbit.calls().len(), 2);
        } else {
            assert_eq!(response["ok"], true, "{response}");
            let path = if mode == "runbook" {
                "knowledge/runbooks/rb-fixture.md"
            } else {
                "knowledge/domain/dom-fixture.md"
            };
            let page = ports.library.read_text(Path::new(path)).unwrap();
            assert!(page.contains(if mode == "update" {
                "Reviewed"
            } else if mode.starts_with("retire") {
                "status: retired"
            } else {
                "Steps based"
            }));
        }
    }
}

#[test]
fn proposal_conformance_goldens_replay_success_and_refusal_offline() {
    for verb in ["proposal-create", "proposal-accept"] {
        let (ports, orbit) = fixture();
        if verb == "proposal-create" {
            orbit.push_response(Ok(json!({"tasks":[],"truncated":false})));
            orbit.push_response(Ok(json!({"id":"proposal-fixture","status":"proposed"})));
            orbit.push_response(Ok(json!({})));
        } else {
            accept_replies(&orbit, &auto_task());
            reserve(&orbit);
            orbit.push_response(Ok(json!({"name":"weekly-report","enabled":true})));
            orbit.push_response(Ok(json!({"name":"weekly-report","enabled":true})));
            close(&orbit);
            orbit.push_response(Ok(json!({"status":"done"})));
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
            for (prefix, ok) in [("      output: ", true), ("      error: ", false)] {
                if let Some(value) = line.strip_prefix(prefix) {
                    let expected: Value = serde_json::from_str(value).unwrap();
                    let response = call(&ports, &format!("office.{verb}"), input.take().unwrap());
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
}

#[test]
fn proposal_accept_requires_the_exact_reviewed_payload_digest() {
    let (ports, orbit) = fixture();
    accept_replies(&orbit, &auto_task());
    let response = call(
        &ports,
        "office.proposal-accept",
        json!({"id":"proposal-fixture","sha256":"0".repeat(64)}),
    );
    assert_eq!(
        response["error"]["code"], "proposal_payload_changed",
        "{response}"
    );
    assert_eq!(orbit.calls().len(), 2);
}

fn approval(payload: &Value) -> Value {
    json!({"id":"proposal-fixture","sha256":office_core::domain::source::sha256_hex(payload.to_string().as_bytes())})
}
