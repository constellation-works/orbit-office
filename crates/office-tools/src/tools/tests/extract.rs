use super::video_plan::{ID, call, fixture};
use office_core::domain::settings::Settings;
use serde_json::json;

#[test]
fn kt_requires_confirmation_before_inference() {
    let (ports, models) = fixture(Settings::default(), true, false);
    let response = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(response["error"]["code"], "plan_unconfirmed", "{response}");
    assert!(models.sensitivities().is_empty());
}

use super::video_plan::{PATH, confirm, private_settings};
use office_core::domain::endpoint::Capability;
use office_core::domain::source::{sha256_hex, video};
use office_core::ports::Library;
use office_testkit::fakes::{FakeLibrary, FakeOrbit};
use serde_json::Value;
use std::path::Path;

fn digest_reply() -> Value {
    json!({"summary":"Digest fixture","kind_hint":"design-doc","as_of":null,
        "subjects":[{"name":"SidecarSubject","template":"topic","aliases":[],"role":"owner"}],
        "claims":[{"subject":"SidecarSubject","text":"Grounded","cites":["#L1-2"],"volatile":false},
                  {"subject":"SidecarSubject","text":"Missing","volatile":false},
                  {"subject":"SidecarSubject","text":"Outside","cites":["#L99999"],"volatile":true}],
        "procedures":[{"title":"Deploy","inputs":[],"steps":[{"text":"Run","cites":["#L2"]},{"text":"No evidence","cites":[]}],"done_when":"Healthy"}],
        "terms":[{"term":"Queue","definition":"Work","cites":["#L1"]},{"term":"Absent","definition":"Bad","cites":["#page=99"]}],
        "refs":[{"label":"Dashboard","url":"https://example.test"}]})
}
fn digest_fixture(
    body: &str,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeModels,
) {
    let library = FakeLibrary::initialized();
    super::batch_plan::source(&library, "src-doc", "docs", body);
    let mut ports = super::batch_plan::ports(library);
    let mut models = office_testkit::fakes::FakeModels::new();
    models.extracted = digest_reply();
    ports.models = Box::new(models.clone());
    (ports, models)
}
#[test]
fn digest_profile_filters_every_cited_item_and_persists_the_shared_shape() {
    let (ports, models) = digest_fixture("# Design\nDeploy safely.");
    super::batch_plan::confirm(&ports, "docs");
    let result = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["output"]["calls"], 1);
    assert_eq!(
        result["output"]["dropped"],
        json!({"uncited":2,"dangling":2,"tangent":0})
    );
    let saved: Value = serde_json::from_str(
        &ports
            .library
            .source_sidecar("src-doc", "digest.json")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved, result["output"]["result"]);
    assert_eq!(saved["claims"].as_array().unwrap().len(), 1);
    assert_eq!(saved["procedures"][0]["steps"].as_array().unwrap().len(), 1);
    assert_eq!(saved["terms"].as_array().unwrap().len(), 1);
    assert_eq!(saved["subjects"][0]["role"], "owner");
    assert!(saved["as_of"].is_null());
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].schema["additionalProperties"], false);
    assert!(requests[0].schema["properties"]["subjects"].is_object());
    assert!(requests[0].text.contains("#L2 Deploy safely."));
    assert_eq!(
        call(&ports, "office.search", json!({"query":"SidecarSubject"}))["output"]["hits"][0]["id"],
        "src-doc"
    );
}

#[test]
fn digest_windows_and_merge_are_included_in_the_confirmed_batch_allowance() {
    let body = format!("# Design\n{}\nTail evidence.", "界".repeat(60_000));
    let (ports, models) = digest_fixture(&body);
    ports
        .library
        .set_setting("costs.currency", json!("USD"))
        .unwrap();
    ports
        .library
        .set_setting("costs.text_1k_tokens", json!(0.01))
        .unwrap();
    let batch = super::batch_plan::confirm(&ports, "docs");
    assert_eq!(batch["plan"]["counts"]["digest"], 3);
    let result = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["output"]["calls"], 3);
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].text.contains("Tail evidence."));
    assert!(requests[1].text.contains("#L3 Tail evidence."));
    assert!(requests[2].text.contains("Merge these window results"));
    assert_eq!(
        requests[..2]
            .iter()
            .map(|r| r.text.chars().filter(|c| *c == '界').count())
            .sum::<usize>(),
        60_000
    );
    assert!(
        requests[..2]
            .iter()
            .all(|r| r.text.chars().count() < 49_000)
    );
    let actual_tokens = requests
        .iter()
        .map(|r| u64::try_from(r.text.chars().count()).unwrap().div_ceil(4))
        .sum::<u64>();
    assert!(batch["plan"]["estimate"]["text_tokens"].as_u64().unwrap() >= actual_tokens);
    assert_eq!(
        ports.library.bootstrap_snapshot().unwrap().batches["docs"].used[0],
        3
    );
}

#[test]
fn digest_requires_confirmation_and_never_retries_a_partly_spent_group() {
    let (mut ports, mut models) = digest_fixture(&format!("# Design\n{}", "x".repeat(60_000)));
    let unconfirmed = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(unconfirmed["error"]["code"], "plan_unconfirmed");
    assert!(models.extract_requests.borrow().is_empty());
    super::batch_plan::confirm(&ports, "docs");
    models.extract_failure = Some(1);
    ports.models = Box::new(models.clone());
    let failed = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(failed["error"]["code"], "model_unavailable");
    assert_eq!(models.extract_requests.borrow().len(), 2);
    assert!(
        ports
            .library
            .source_sidecar("src-doc", "digest.json")
            .unwrap()
            .is_none()
    );
    let window = ports
        .library
        .read_text(Path::new(
            "sources/1970/01/src-doc/extract-window-0000.json",
        ))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&window).unwrap()["claims"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let retry = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(retry["error"]["code"], "plan_exceeded");
    assert_eq!(models.extract_requests.borrow().len(), 2);
}

#[test]
fn digest_publication_preserves_other_documents_allowances_and_private_routing() {
    let mut library = FakeLibrary::initialized();
    library.settings = private_settings(&Capability::ALL);
    for id in ["one", "two"] {
        super::batch_plan::source(&library, id, "docs", "# Design\nEvidence.");
    }
    let mut ports = super::batch_plan::ports(library);
    let mut models = office_testkit::fakes::FakeModels::new();
    models.extracted = digest_reply();
    ports.models = Box::new(models.clone());
    let preview =
        super::batch_plan::preview(&ports, json!({"phase":"docs","sensitivity":"private"}));
    super::batch_plan::preview(
        &ports,
        json!({"phase":"docs","confirm":true,"digest":preview["plan"]["digest"]}),
    );
    for id in ["one", "two"] {
        let result = call(
            &ports,
            "office.extract",
            json!({"id":id,"profile":"digest"}),
        );
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["output"]["derived_by"]["route"], "private:company");
    }
    assert_eq!(models.extract_requests.borrow().len(), 2);
    assert!(
        models
            .sensitivities()
            .iter()
            .all(|s| s.as_str() == "private")
    );
}

#[test]
fn digest_refuses_changed_approved_sources_before_inference() {
    let (ports, models) = digest_fixture("# Design\nEvidence.");
    super::batch_plan::confirm(&ports, "docs");
    let path = Path::new("sources/1970/01/src-doc/source.md");
    let old = ports.library.read_text(path).unwrap();
    ports
        .library
        .write_document(path, &format!("{old}Changed.\n"))
        .unwrap();
    let result = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(result["error"]["code"], "plan_changed", "{result}");
    assert!(models.extract_requests.borrow().is_empty());
}

pub(super) fn reply() -> Value {
    json!({"summary":"Fixture KT",
        "chapters":[{"title":"Deploy","label":"core","start":0,"end":60},{"title":"Aside","label":"tangent","start":60,"end":120}],
        "gotchas":[{"text":"Avoid the trap","cites":["#t=00:01:10"]}],
        "knowledge_candidates":[{"template":"topic","target_page":"Deployment","claims":[
            {"text":"Useful","cites":["#t=00:00:00"]}, {"text":"uncited","cites":[]},
            {"text":"dangling","cites":["#t=99:00:00"]}, {"text":"tangent","cites":["#t=00:01:10"]}]}],
        "runbook_candidates":[{"title":"Deploy","inputs":["branch"],"steps":[{"text":"Run deploy","cites":["#frame=00:00:20"]}],"done_when":"Healthy"}],
        "glossary":[{"term":"KT","definition":"Knowledge transfer","cites":["#t=00:00:00"]}],
        "questions":[{"text":"Who deploys?","cites":["#t=00:00:00"]}]})
}
fn setup(
    duration_ms: u64,
    private: bool,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeModels,
    FakeOrbit,
) {
    let settings = if private {
        private_settings(&Capability::ALL)
    } else {
        Settings::default()
    };
    let (mut ports, mut models) = fixture(settings, true, private);
    let source = ports.library.read_video(ID).unwrap();
    ports
        .library
        .write_document(
            Path::new(PATH),
            &source.markdown.replace(
                "Transcript.",
                "[00:00:00] Ada: Deploy.\n\n[00:01:10] Ben: An aside.\n\n[00:16:00] Ada: Later.",
            ),
        )
        .unwrap();
    models.video_analysis.as_mut().unwrap().duration_ms = duration_ms;
    models.extracted = reply();
    ports.models = Box::new(models.clone());
    let orbit = FakeOrbit::default();
    for _ in 0..2 {
        orbit.push_response(Ok(json!({"id":"task-fixture"})));
    }
    ports.orbit = Box::new(orbit.clone());
    confirm(&ports);
    let frames = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(frames["ok"], true, "{frames}");
    (ports, models, orbit)
}
#[test]
fn kt_filters_claims_writes_digest_and_chapters_and_files_only_questions() {
    let (ports, models, orbit) = setup(120_000, false);
    let output = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(output["ok"], true, "{output}");
    let output = &output["output"];
    assert_eq!(
        output["dropped"],
        json!({"uncited":1,"dangling":1,"tangent":1})
    );
    assert_eq!(
        output["result"]["knowledge_candidates"][0]["claims"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let chapters = ports
        .library
        .source_sidecar(ID, "chapters.md")
        .unwrap()
        .unwrap();
    assert!(chapters.contains("[core]") && chapters.contains("[tangent]"));
    let digest: Value = serde_json::from_str(
        &ports
            .library
            .source_sidecar(ID, "digest.json")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    office_core::domain::bootstrap::digest(&digest).unwrap();
    assert_eq!(digest["claims"].as_array().unwrap().len(), 2);
    assert_eq!(
        digest["procedures"][0]["steps"][0]["cites"],
        json!(["#frame=00:00:20"])
    );
    let calls = orbit.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].tool, "orbit.task.add");
    assert_eq!(
        calls[0].input["tags"],
        json!(["owner:you", format!("source:{ID}"), "kt-question"])
    );
    assert_eq!(calls[1].tool, "orbit.task.artifact.put");
    assert_eq!(calls[1].input["id"], "task-fixture");
    let requests = models.extract_requests.borrow();
    assert!(
        requests[0].text.find("#frame=00:00:20").unwrap()
            < requests[0].text.find("#t=00:01:10").unwrap()
    );
    assert_eq!(requests[0].schema["additionalProperties"], false);
    assert_eq!(
        video::state(&ports.library.read_video(ID).unwrap().markdown)
            .unwrap()
            .unwrap()
            .calls
            .extract,
        1
    );
    assert_eq!(
        call(
            &ports,
            "office.extract",
            json!({"id":ID,"profile":"kt","task":"task-fixture"})
        )["error"]["code"],
        "plan_exceeded"
    );
}
#[test]
fn long_private_timeline_uses_fifteen_minute_windows_and_one_merge() {
    let (ports, models, _) = setup(1_800_000, true);
    let result = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].text.contains("Ada: Deploy"));
    assert!(!requests[0].text.contains("Ada: Later"));
    assert!(requests[1].text.contains("Ada: Later"));
    assert!(!requests[1].text.contains("Ada: Deploy"));
    assert!(requests[2].text.contains("Merge these window results"));
    assert!(
        models
            .sensitivities()
            .iter()
            .all(|s| s.as_str() == "private")
    );
    assert_eq!(result["output"]["calls"], 3);
}
#[test]
fn meeting_files_owned_todos_and_attaches_the_structured_result() {
    let (mut ports, mut models) = fixture(Settings::default(), false, false);
    let library = FakeLibrary::initialized();
    library.write_document(Path::new(PATH), &format!("---\nid: {ID}\nkind: meeting\ntitle: Standup\nsensitivity: private\n---\n[00:00:00] Ada: Ship.\n")).unwrap();
    let staging = library.staging();
    ports.library = Box::new(library);
    models.extracted = json!({"summary":"Ship","decisions":[{"text":"Ship","cites":["#t=00:00:00"]}],"to_dos":[{"text":"Review","owner":"you","cites":["#t=00:00:00"]},{"text":"Draft","owner":"agent","cites":["#L1"]}]});
    ports.models = Box::new(models.clone());
    let orbit = FakeOrbit::default();
    for _ in 0..3 {
        orbit.push_response(Ok(json!({"id":"task-fixture"})));
    }
    ports.orbit = Box::new(orbit.clone());
    let result = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting","task":"task-fixture"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let calls = orbit.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0].input["tags"],
        json!(["owner:you", format!("source:{ID}")])
    );
    // Regression (ORB-19747): a model reply marking a to-do `owner:agent`
    // must not file it as an agent to-do. Extraction files every meeting
    // to-do as `owner:you`; promotion to `owner:agent` (with
    // `no-diff-expected` and acceptance criteria) is the orchestrator's step.
    assert_eq!(
        calls[1].input["tags"],
        json!(["owner:you", format!("source:{ID}")])
    );
    for call in calls.iter().filter(|c| c.tool == "orbit.task.add") {
        let tags = call.input["tags"].as_array().unwrap();
        assert!(
            !tags.iter().any(|t| t.as_str() == Some("owner:agent")),
            "filed meeting to-do must never carry owner:agent: {call:?}"
        );
    }
    let staged = calls[2].input["source_path"].as_str().unwrap();
    let artifact: Value = serde_json::from_str(&staging.text(staged).unwrap()).unwrap();
    assert_eq!(artifact, result["output"]);
    assert!(staging.live().is_empty());
    assert_eq!(models.sensitivities()[0].as_str(), "private");
}
#[test]
fn an_uncertain_extract_reservation_refuses_a_retry_before_inference() {
    let (ports, models, _) = setup(120_000, false);
    let source = ports.library.read_video(ID).unwrap();
    let state = video::state(&source.markdown).unwrap().unwrap();
    ports
        .library
        .mutate_video(
            ID,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::ReserveProcessing {
                digest: state.plan.digest,
                group: "extract".into(),
                index: 0,
            },
        )
        .unwrap();
    let result = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(result["error"]["code"], "plan_exceeded");
    assert!(models.extract_requests.borrow().is_empty());
}

#[test]
fn transcribe_and_extract_conformance_goldens_replay_offline_success_and_refusal() {
    for verb in ["transcribe", "extract"] {
        let ports = if verb == "transcribe" {
            fixture(Settings::default(), true, false).0
        } else {
            setup(120_000, false).0
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(&path).unwrap();
        let mut input = None;
        let mut lines = Vec::new();
        let mut counts = (0, 0);
        for line in golden.lines() {
            if let Some(raw) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<Value>(raw).unwrap());
            }
            let mut replacement = None;
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(raw) = line.strip_prefix(prefix) {
                    let expected: Value = serde_json::from_str(raw).unwrap();
                    let input = input.take().unwrap();
                    let response = if verb == "extract" && input["profile"] == "digest" {
                        let (ports, _) = digest_fixture("# Design\nDeploy safely.");
                        super::batch_plan::confirm(&ports, "docs");
                        call(&ports, "office.extract", input)
                    } else {
                        call(&ports, &format!("office.{verb}"), input)
                    };
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        counts.0 += 1;
                        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
                            replacement = Some(format!("{prefix}{}", response["output"]));
                        } else {
                            assert_eq!(response["output"], expected);
                        }
                    } else {
                        counts.1 += 1;
                        assert_eq!(response["error"]["code"], expected["code"]);
                    }
                }
            }
            lines.push(replacement.unwrap_or_else(|| line.into()));
        }
        assert!(counts.0 > 0 && counts.1 > 0);
        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
            std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
        }
    }
}

#[test]
fn failed_second_window_preserves_first_and_spends_no_merge_or_retry_calls() {
    let (mut ports, mut models, orbit) = setup(1_800_000, false);
    models.extract_failure = Some(1);
    ports.models = Box::new(models.clone());
    let response = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(response["error"]["code"], "model_unavailable");
    let path = Path::new(PATH).with_file_name("extract-window-0000.json");
    let saved: Value = serde_json::from_str(&ports.library.read_text(&path).unwrap()).unwrap();
    assert_eq!(
        saved["knowledge_candidates"][0]["claims"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        ports
            .library
            .source_sidecar(ID, "digest.json")
            .unwrap()
            .is_none()
    );
    assert!(orbit.calls().is_empty());
    assert_eq!(models.extract_requests.borrow().len(), 2);
    let retry = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(retry["error"]["code"], "plan_exceeded");
    assert_eq!(models.extract_requests.borrow().len(), 2);
}

#[test]
fn uncited_missing_and_tangent_steps_and_terms_are_all_dropped_and_counted() {
    let (mut ports, mut models, _) = setup(120_000, false);
    models.extracted["runbook_candidates"][0]["steps"] = json!([
        {"text":"No cite","cites":[]},
        {"text":"Bad frame","cites":["#frame=00:04:00"]},
        {"text":"Aside step","cites":["#t=00:01:10"]}
    ]);
    models.extracted["glossary"] =
        json!([{ "term":"Aside", "definition":"Only a tangent", "cites":["#t=00:01:10"] }]);
    ports.models = Box::new(models);
    let result = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        result["output"]["dropped"],
        json!({"uncited":2,"dangling":2,"tangent":3})
    );
    assert_eq!(result["output"]["result"]["runbook_candidates"], json!([]));
    assert_eq!(result["output"]["result"]["glossary"], json!([]));
}

#[test]
fn merge_cannot_promote_window_tangents_to_ordinary_knowledge() {
    let (ports, models, _) = setup(1_800_000, false);
    let mut merged = reply();
    merged["chapters"][1]["label"] = json!("core");
    models
        .extract_replies
        .borrow_mut()
        .extend([reply(), reply(), merged]);
    let result = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        result["output"]["result"]["knowledge_candidates"][0]["claims"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result["output"]["dropped"]["tangent"], 3);
}

#[test]
fn a_meeting_decision_enters_the_ledger_as_its_plain_statement_cited_at_its_turn() {
    // ORB-21377: the model's text ends in an inline cite and the cited line is
    // a transcribed turn ending in its own link; neither may reach `chosen`.
    let body = "# Sync\n\n[00:00:40] B: Pages stay at fifty. [turn](#t=00:00:40)\n[00:01:03] A: Decision. We rotate ingest keys monthly starting in December. The weekly schedule costs more than it protects. [turn](#t=00:01:03)\n";
    let item =
        json!({"text":"We rotate ingest keys monthly starting in December [#L4]","cites":["#L4"]});
    let input = crate::tools::sources::extract::ledger::entry_input(
        "src-20261102-sync",
        body,
        &item,
        "2026-11-02T09:00:00Z",
    )
    .expect("the cited turn resolves");
    assert_eq!(
        input["chosen"],
        "We rotate ingest keys monthly starting in December"
    );
    assert_eq!(
        input["question"],
        "We rotate ingest keys monthly starting in December"
    );
    assert_eq!(input["ref"], "src-20261102-sync#t=00:01:03");
    assert!(
        input["quote"]
            .as_str()
            .unwrap()
            .starts_with("[00:01:03] A: Decision.")
    );
}

/// A transcribed meeting (`[hh:mm:ss] Speaker: … [turn](#t=…)` turns) and
/// its fixture ports, with every Orbit call answered.
fn transcribed_meeting(
    reply: Value,
    orbit_replies: Vec<office_core::Result<Value>>,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeModels,
) {
    let (mut ports, mut models) = fixture(Settings::default(), false, false);
    let library = FakeLibrary::initialized();
    library.write_document(Path::new(PATH), &format!("---\nid: {ID}\nkind: meeting\ntitle: Sync\nsensitivity: normal\n---\n[00:00:00] A: Morning. [turn](#t=00:00:00)\n\n[00:00:49] A: Decision. We keep fifty orders per page. [turn](#t=00:00:49)\n\n[00:01:10] B: I'll tell the access desk. [turn](#t=00:01:10)\n")).unwrap();
    ports.library = Box::new(library);
    models.extracted = reply;
    ports.models = Box::new(models.clone());
    let orbit = FakeOrbit::default();
    for reply in orbit_replies {
        orbit.push_response(reply);
    }
    ports.orbit = Box::new(orbit);
    (ports, models)
}

#[test]
fn meeting_extract_and_reconcile_outputs_conform_to_the_declared_response_schema() {
    // ORB-21415: #354 added `filing` (and reconcile's output) without the
    // schema, so Orbit refused every meeting extract's output.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/schemas/extract.response.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let (ports, _) = transcribed_meeting(
        json!({"summary":"Sync",
        "decisions":[{"text":"Keep fifty orders per page","cites":["#t=00:00:49"]}],
        "to_dos":[{"text":"Tell the access desk","owner":"you","cites":["#t=00:01:10"]}]}),
        // Open conflicts, then the to-do's filing fails; reconcile lists
        // none existing and files it.
        vec![
            Ok(json!({"tasks":[]})),
            Err(office_core::Error::OrbitOutcomeUnknown),
            Ok(json!({"tasks":[]})),
            Ok(json!({"id":"task-fixture"})),
        ],
    );
    let extracted = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting"}),
    );
    assert_eq!(extracted["ok"], true, "{extracted}");
    let reconciled = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting","reconcile":true}),
    );
    assert_eq!(reconciled["ok"], true, "{reconciled}");
    for output in [&extracted["output"], &reconciled["output"]] {
        let errors: Vec<String> = validator
            .iter_errors(output)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{errors:?}\n{output}");
    }
}

#[test]
fn meeting_items_citing_copied_turn_links_are_kept_and_an_all_dropped_reply_holds_the_meeting() {
    // ORB-21416: a hosted reply cited in the transcript's own link form
    // (`[turn](#t=…)`, `[#L…]`); every item failed the exact anchor match and
    // the meeting was marked processed with no decisions or to-dos.
    let (ports, _) = transcribed_meeting(
        json!({"summary":"Sync [turn](#t=00:00:00)",
        "decisions":[{"text":"Keep fifty orders per page","cites":["[turn](#t=00:00:49)"]}],
        "to_dos":[{"text":"Tell the access desk","owner":"you","cites":["[#L5]","t=00:01:10"]}]}),
        vec![Ok(json!({"tasks":[]})), Ok(json!({"id":"task-fixture"}))],
    );
    let output = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting"}),
    );
    assert_eq!(output["ok"], true, "{output}");
    let result = &output["output"]["result"];
    assert_eq!(result["decisions"][0]["cites"], json!(["#t=00:00:49"]));
    assert_eq!(result["to_dos"][0]["cites"], json!(["#L5", "#t=00:01:10"]));
    assert_eq!(output["output"]["dropped"]["dangling"], 0);

    let (ports, _) = transcribed_meeting(
        json!({"summary":"Sync",
        "decisions":[{"text":"Keep fifty orders per page","cites":["#t=09:00:00"]}],"to_dos":[]}),
        Vec::new(),
    );
    let held = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting"}),
    );
    assert_eq!(
        held["error"]["code"], "meeting_extract_dropped_all",
        "{held}"
    );
    assert!(
        ports
            .library
            .source_sidecar(ID, "extract-result.json")
            .unwrap()
            .is_none()
    );
    assert!(
        ports
            .library
            .source_sidecar(ID, "summary.md")
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_recorded_hosted_meeting_feeds_every_decision_its_stuttered_turn_supports() {
    // ORB-21431: the box run at 1f3c3be. Muse Voice transcribed the monthly
    // decision as "monthly, monthly. Starting in December", Muse Spark said
    // "monthly starting in December", and the ledger skipped it as
    // quote_mismatch, so the September "weekly" contradiction never surfaced.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lantern-box-1f3c3be");
    let body = std::fs::read_to_string(dir.join("transcript.md")).unwrap();
    let mut reply: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("reply.json")).unwrap()).unwrap();
    // Not said in that turn: "weekly" is there, but not as this statement.
    reply["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"text":"We rotate ingest keys weekly","cites":["#t=00:01:03"]}));
    let (mut ports, models) = fixture(Settings::default(), false, false);
    let library = FakeLibrary::initialized();
    library.write_document(Path::new(PATH), &format!("---\nid: {ID}\nkind: meeting\ntitle: SIMULATED Lantern engineering sync\nsensitivity: normal\n---\n{body}")).unwrap();
    let weekly = json!({"id":"dec-global-1","question":"How often do we rotate ingest keys?","question_by":"scribe",
        "options":"unrecoverable","chosen":"rotate ingest keys weekly","quote":"rotate ingest keys weekly",
        "ref":"src-20260921-synthetic-security-review#L4","scope":"global","decided_by":"human",
        "context":{"task_type":null,"workspace":null,"task":null,"risk_tier":null},
        "decided_at":"2026-09-21T16:00:00Z","reverses":null,"source":"src-20260921-synthetic-security-review"});
    library.write_document(Path::new("sources/decisions/global.md"), &format!("---\nid: decisions-global\nkind: decisions\nscope: global\ncreated_at: 2026-09-21T16:00:00Z\n---\n- dec-global-1 2026-09-21T16:00:00Z #decision {weekly}\n")).unwrap();
    ports.library = Box::new(library);
    models.extract_replies.borrow_mut().extend([
        reply,
        json!({"conflicts":[{"new":"dec-global-3","prior":"dec-global-1","matter":"ingest key rotation"}],"held":[]}),
    ]);
    ports.models = Box::new(models);
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks":[]})));
    for _ in 0..16 {
        orbit.push_response(Ok(json!({"id":"task-fixture"})));
    }
    ports.orbit = Box::new(orbit.clone());
    let output = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"meeting"}),
    );
    assert_eq!(output["ok"], true, "{output}");
    let ledger = &output["output"]["ledger"];
    assert_eq!(
        ledger["appended"],
        json!([
            "dec-global-2",
            "dec-global-3",
            "dec-global-4",
            "dec-global-5"
        ]),
        "{ledger}"
    );
    assert_eq!(
        ledger["skipped"],
        json!([{"index":4,"code":"quote_mismatch"}])
    );
    assert_eq!(
        ledger["conflicts"][0]["entries"],
        json!(["dec-global-1", "dec-global-3"])
    );
    let conflict = orbit
        .calls()
        .into_iter()
        .find(|c| {
            c.tool == "orbit.task.add"
                && c.input["title"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("Which decision stands")
        })
        .expect("the monthly-vs-weekly conflict is filed");
    assert_eq!(
        conflict.input["title"],
        "Which decision stands: \"rotate ingest keys weekly\" or \"We rotate ingest keys monthly starting in December\"?"
    );
}
