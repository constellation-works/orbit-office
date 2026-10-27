//! Daily digests through the public dispatch boundary, without network or host state.

use office_core::domain::artifact::Artifact;
use office_core::ports::{Library, Ports};
use office_testkit::fakes::{FakeLibrary, FakeModels, FakeOrbit, PortsBuilder};
use serde_json::{Value, json};

fn serve(ports: &Ports<'_>, input: Value) -> Value {
    let raw = office_testkit::envelope::request("office.digest", input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request");
    serde_json::from_str(&crate::serve(&request, ports)).expect("response")
}

fn fixture(library: FakeLibrary, reply: Value) -> (Ports<'static>, FakeModels, FakeOrbit) {
    let mut models = FakeModels::new();
    models.extracted = reply;
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks":[],"total":0,"truncated":false})));
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.models = Box::new(models.clone());
    ports.orbit = Box::new(orbit.clone());
    (ports, models, orbit)
}

fn conversation(library: &FakeLibrary, sensitivity: &str) {
    library.write_document(std::path::Path::new("sources/2026/10/src-20261026-session/source.md"),
        &format!("---\nid: src-20261026-session\nkind: conversation\ntitle: Session\nsensitivity: {sensitivity}\n---\n[2026-10-26T10:00:00Z] decision: Ship the renewal draft.\n[2026-10-27T10:00:00Z] decision: Tomorrow only.\n")).expect("source");
}

#[test]
fn digest_empty_day_and_invalid_day_never_spend_or_write() {
    let (ports, models, orbit) = fixture(FakeLibrary::initialized(), json!({"lines":[]}));
    let response = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["reason"], "no_activity");
    for input in [
        json!({"day":"2026-02-30"}),
        json!({"day":3}),
        json!({"day":"9223372036854775807-12-31"}),
        json!({"day":"2026-10-26","extra":true}),
    ] {
        assert_eq!(serve(&ports, input)["error"]["code"], "invalid_argument");
    }
    assert!(models.extract_requests.borrow().is_empty());
    assert!(
        ports
            .library
            .artifact_documents()
            .expect("artifacts")
            .is_empty()
    );
    assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.list"));
}

#[test]
fn digest_one_call_cited_report_omits_unsupported_lines_and_rerun_is_noop() {
    let library = FakeLibrary::initialized();
    conversation(&library, "normal");
    let (ports, models, orbit) = fixture(
        library,
        json!({"lines":[
            {"evidence":0,"text":"Ship the renewal draft."},
            {"evidence":0,"text":"Invented unsupported claim."},
            {"evidence":999,"text":"Unknown evidence."}
        ]}),
    );
    let response = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["written"], true);
    assert_eq!(models.extract_requests.borrow().len(), 1);
    let files = ports.library.artifact_documents().expect("artifacts");
    assert_eq!(files.len(), 1);
    let artifact =
        Artifact::from_markdown(files[0].markdown.as_deref().expect("markdown")).expect("artifact");
    assert_eq!(artifact.series, "digest-daily");
    assert_eq!(
        serde_json::to_value(&artifact.approved).expect("approval")["by"],
        "routine"
    );
    assert!(!artifact.approved.edited);
    let path = format!(
        "{}/digest.md",
        artifact
            .path()
            .expect("path")
            .trim_end_matches("/artifact.md")
    );
    let body = String::from_utf8(
        ports
            .library
            .read_artifact_file(std::path::Path::new(&path))
            .expect("body"),
    )
    .expect("UTF8");
    assert_eq!(
        body,
        "- Ship the renewal draft. [src-20261026-session#L1]\n"
    );
    assert!(
        !models.extract_requests.borrow()[0]
            .text
            .contains("Tomorrow only")
    );
    let again = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(again["output"]["reason"], "already_exists", "{again}");
    assert_eq!(models.extract_requests.borrow().len(), 1);
    assert_eq!(
        ports.library.artifact_documents().expect("artifacts").len(),
        1
    );
    assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.list"));
}

fn report_body(ports: &Ports<'_>, response: &Value) -> String {
    let path = response["output"]["path"].as_str().expect("artifact path");
    let path = format!("{}/digest.md", path.trim_end_matches("/artifact.md"));
    String::from_utf8(
        ports
            .library
            .read_artifact_file(std::path::Path::new(&path))
            .expect("report"),
    )
    .expect("UTF8")
}

#[test]
fn digest_private_route_and_unconfigured_reference_only_are_safe() {
    for configured in [false, true] {
        let mut library = FakeLibrary::initialized();
        if configured {
            library.settings = super::video_plan::private_settings(&[
                office_core::domain::endpoint::Capability::Text,
            ]);
        }
        conversation(&library, "private");
        let (ports, models, orbit) = fixture(
            library,
            json!({"lines":[{"evidence":0,"text":"Ship the renewal draft."}]}),
        );
        let response = serve(&ports, json!({"day":"2026-10-26"}));
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(models.extract_requests.borrow().len(), 1);
        let input = models.extract_requests.borrow()[0].text.clone();
        let body = report_body(&ports, &response);
        if configured {
            assert_eq!(models.sensitivities()[0].as_str(), "private");
            assert!(input.contains("Ship the renewal draft."));
            assert!(body.contains("Ship the renewal draft."));
        } else {
            assert_eq!(models.sensitivities()[0].as_str(), "normal");
            assert!(!input.contains("Ship the renewal draft."));
            assert_eq!(
                body,
                "- Private item (reference only). [src-20261026-session#L1]\n"
            );
        }
        let text = ports.library.artifact_documents().expect("artifacts");
        assert_eq!(
            Artifact::from_markdown(text[0].markdown.as_deref().expect("text"))
                .expect("artifact")
                .sensitivity
                .as_str(),
            "private"
        );
        assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.list"));
    }
}

#[test]
fn digest_knowledge_reaches_models_only_as_a_model_view() {
    use office_core::domain::knowledge::Revision;
    let library = FakeLibrary::initialized();
    conversation(&library, "normal");
    let path = std::path::Path::new("knowledge/domain/dom-person-jane.md");
    library.write_document(path, "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\nrefs:\n  - { label: Work email, contact: secret-contact@example.test }\ncites: [src-20261026-session#L1]\n---\n## Role\nBuyer.\nOld contact secret-contact@example.test\n").expect("page");
    library
        .set_history(
            path,
            vec![Revision {
                commit: "fixture".into(),
                author: "Fixture".into(),
                email: "fixture@example.test".into(),
                authored_at: "2026-10-26T10:00:00Z".into(),
                message: "page changed".into(),
            }],
        )
        .expect("history");
    let (ports, models, _) = fixture(library, json!({"lines":[{"evidence":1,"text":"Buyer."}]}));
    let response = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(response["ok"], true, "{response}");
    let input: Value =
        serde_json::from_str(&models.extract_requests.borrow()[0].text).expect("input");
    let view: Value = serde_json::from_str(input["evidence"][1]["text"].as_str().expect("view"))
        .expect("view JSON");
    assert_eq!(view["refs"][0]["label"], "Work email");
    assert_eq!(view["refs"][0]["value"], Value::Null);
    assert!(
        !models.extract_requests.borrow()[0]
            .text
            .contains("secret-contact@example.test")
    );
    assert!(!report_body(&ports, &response).contains("secret-contact@example.test"));
}

#[test]
fn digest_conformance_replays_both_cases_without_network_or_secrets() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/digest.yaml");
    let golden = std::fs::read_to_string(path).expect("golden");
    let mut input = None;
    let mut cases = [0, 0];
    for line in golden.lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).expect("input"));
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).expect("expected");
                let (ports, models, orbit) =
                    fixture(FakeLibrary::initialized(), json!({"lines":[]}));
                let response = serve(&ports, input.take().expect("input"));
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                    cases[0] += 1;
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                    cases[1] += 1;
                }
                assert!(models.extract_requests.borrow().is_empty());
                assert!(
                    ports
                        .library
                        .artifact_documents()
                        .expect("artifacts")
                        .is_empty()
                );
                assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.list"));
            }
        }
    }
    assert_eq!(cases, [1, 1]);
}

#[test]
fn digest_gathers_completed_work_proposals_and_approved_artifacts_for_only_the_day() {
    use office_core::domain::artifact::{Approval, Approver, ArtifactManifest, Producer};
    let library = FakeLibrary::initialized();
    let manifest: ArtifactManifest = serde_json::from_value(
        json!({"kind":"report","title":"Renewal report","files":["report.md"],"series":"renewals"}),
    )
    .expect("manifest");
    let artifact = Artifact::approved(
        manifest,
        "fixture-task".into(),
        Producer {
            crew: None,
            runbook: None,
            at: None,
        },
        Approval {
            by: Approver::User,
            at: "2026-10-26T11:00:00Z".into(),
            edited: false,
        },
    )
    .expect("artifact");
    library
        .promote_artifact(
            &artifact,
            &[office_core::domain::artifact::ArtifactFile {
                path: "report.md".into(),
                bytes: b"Report".to_vec(),
            }],
        )
        .expect("seed artifact");
    let (mut ports, models, _) = fixture(
        library,
        json!({"lines":[
            {"evidence":0,"text":"Task completed: Completed"},
            {"evidence":1,"text":"Proposal accepted: Accepted"},
            {"evidence":2,"text":"Proposal dismissed: Dismissed"},
            {"evidence":3,"text":"Artifact approved: Renewal report"}
        ]}),
    );
    let task = |id, title, tags: Value, status, at| json!({"id":id,"title":title,"tags":tags,"status":status,"type":"chore","created_at":"2026-10-01T00:00:00Z","history":[{"at":at,"from_status":"review","to_status":status}],"artifacts":[]});
    let tasks = json!([
        task(
            "a-task",
            "Completed",
            json!(["owner:agent"]),
            "done",
            "2026-10-26T12:00:00Z"
        ),
        task(
            "b-proposal",
            "Accepted",
            json!(["proposal"]),
            "done",
            "2026-10-26T13:00:00Z"
        ),
        task(
            "c-proposal",
            "Dismissed",
            json!(["proposal"]),
            "rejected",
            "2026-10-26T14:00:00Z"
        ),
        task(
            "d-old",
            "Outside day",
            json!(["owner:you"]),
            "done",
            "2026-10-25T14:00:00Z"
        )
    ]);
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks":tasks,"total":4,"truncated":false})));
    ports.orbit = Box::new(orbit.clone());
    let response = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(response["ok"], true, "{response}");
    let input = models.extract_requests.borrow()[0].text.clone();
    for text in ["Completed", "Accepted", "Dismissed", "Renewal report"] {
        assert!(input.contains(text), "{input}");
    }
    assert!(!input.contains("Outside day"));
    let body = report_body(&ports, &response);
    assert_eq!(body.lines().count(), 4);
    for id in ["a-task", "b-proposal", "c-proposal", "fixture-task"] {
        assert!(body.contains(&format!("[{id}]")));
    }
    assert_eq!(models.extract_requests.borrow().len(), 1);
    assert!(orbit.calls().iter().all(|c| c.tool == "orbit.task.list"));
}

#[test]
fn digest_refuses_incomplete_evidence_and_model_failure_without_writes() {
    for truncated in [false, true] {
        let library = FakeLibrary::initialized();
        conversation(&library, "normal");
        let (mut ports, mut models, _) = fixture(library, json!({"lines":[]}));
        if truncated {
            let orbit = FakeOrbit::default();
            orbit.push_response(Ok(json!({"tasks":[],"total":1,"truncated":true})));
            ports.orbit = Box::new(orbit);
        } else {
            models.extract_failure = Some(0);
            ports.models = Box::new(models.clone());
        }
        let response = serve(&ports, json!({"day":"2026-10-26"}));
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(
            response["error"]["code"],
            if truncated {
                "task_list_truncated"
            } else {
                "model_unavailable"
            }
        );
        assert_eq!(
            models.extract_requests.borrow().len(),
            usize::from(!truncated)
        );
        assert!(
            ports
                .library
                .artifact_documents()
                .expect("artifacts")
                .is_empty()
        );
    }
}

#[test]
fn digest_default_day_is_the_previous_complete_utc_day() {
    let (mut ports, models, _) = fixture(FakeLibrary::initialized(), json!({"lines":[]}));
    ports.clock = Box::new(office_testkit::fakes::FakeClock(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(86400),
    ));
    let response = serve(&ports, json!({}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["day"], "1970-01-01");
    assert!(models.extract_requests.borrow().is_empty());
}

#[test]
fn digest_drops_excerpts_that_remove_negation_or_context() {
    let library = FakeLibrary::initialized();
    library.write_document(std::path::Path::new("sources/2026/10/src-20261026-session/source.md"),
        "---\nid: src-20261026-session\nkind: conversation\ntitle: Session\nsensitivity: normal\n---\n[2026-10-26T10:00:00Z] decision: Do not ship the renewal draft.\n").expect("source");
    let (ports, models, _) = fixture(
        library,
        json!({"lines":[{"evidence":0,"text":"ship the renewal draft."}]}),
    );
    let response = serve(&ports, json!({"day":"2026-10-26"}));
    assert_eq!(
        response["output"]["reason"], "no_supported_lines",
        "{response}"
    );
    assert!(
        ports
            .library
            .artifact_documents()
            .expect("artifacts")
            .is_empty()
    );
    assert_eq!(models.extract_requests.borrow().len(), 1);
}
