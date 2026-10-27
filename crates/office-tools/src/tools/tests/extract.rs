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
    assert_eq!(
        calls[1].input["tags"],
        json!(["owner:agent", format!("source:{ID}")])
    );
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
