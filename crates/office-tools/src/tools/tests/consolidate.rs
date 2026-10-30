//! Consolidation through the same dispatcher as plugin calls, with offline ports.
use super::batch_plan::{digest, source};
use super::video_plan::call;
use office_core::domain::bootstrap::{self, Mutation};
use office_core::ports::{Library, Ports};
use office_testkit::fakes::{FakeLibrary, FakeModels, FakeOrbit, PortsBuilder};
use serde_json::{Value, json};

fn fixture(digests: Vec<Value>) -> (Ports<'static>, FakeModels, FakeOrbit) {
    configured(digests, |_| {})
}

fn configured(
    digests: Vec<Value>,
    configure: impl FnOnce(&FakeLibrary),
) -> (Ports<'static>, FakeModels, FakeOrbit) {
    let library = FakeLibrary::with_bundled_templates();
    for index in 0..digests.len() {
        source(
            &library,
            &format!("src-{index}"),
            "docs",
            "# Ingest\nEvidence.\nMore evidence.",
        );
    }
    configure(&library);
    let settings = library.settings().unwrap();
    let snapshot = library.bootstrap_snapshot().unwrap();
    let plan = bootstrap::plan(&snapshot, &settings, "docs", false, vec![]).unwrap();
    let approved = plan.digest.clone();
    library
        .mutate_batch(
            &snapshot.revision(&settings).unwrap(),
            Mutation::Plan {
                plan: Box::new(plan.clone()),
                confirm: None,
                at: "1970-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
    let snapshot = library.bootstrap_snapshot().unwrap();
    library
        .mutate_batch(
            &snapshot.revision(&settings).unwrap(),
            Mutation::Plan {
                plan: Box::new(plan),
                confirm: Some(approved),
                at: "1970-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
    for (index, value) in digests.into_iter().enumerate() {
        library
            .insert_digest(&format!("src-{index}"), value)
            .unwrap();
    }
    let models = FakeModels::new();
    let orbit = FakeOrbit::default();
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.models = Box::new(models.clone());
    ports.orbit = Box::new(orbit.clone());
    (ports, models, orbit)
}

fn proposed(orbit: &FakeOrbit, id: &str) {
    orbit.push_response(Ok(json!({"tasks":[],"truncated":false})));
    orbit.push_response(Ok(json!({"id":id,"status":"proposed"})));
    orbit.push_response(Ok(json!({})));
}

#[test]
fn four_sources_one_subject_file_one_proposal_and_single_source_is_below_bar() {
    let mut values = vec![digest(json!([{"name":"Ingest","template":"topic"} ])); 4];
    values[0]["subjects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"One off","template":"topic"}));
    let (ports, models, orbit) = fixture(values);
    models
        .extract_replies
        .borrow_mut()
        .push_back(json!({"body":"## Overview\nIngest design [source](src-0#L2).","conflicts":[]}));
    proposed(&orbit, "proposal-ingest");
    let result = call(&ports, "office.consolidate", json!({"batch_id":"docs"}));
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["output"]["failed"], json!([]), "{result}");
    assert_eq!(result["output"]["proposed"].as_array().unwrap().len(), 1);
    assert_eq!(result["output"]["below_bar"], json!(["one off"]));
    assert_eq!(models.extract_requests.borrow().len(), 1);
    assert_eq!(
        orbit
            .calls()
            .iter()
            .filter(|c| c.tool == "orbit.task.add")
            .count(),
        1
    );
}

fn body(models: &FakeModels, body: &str) {
    models
        .extract_replies
        .borrow_mut()
        .push_back(json!({"body":body,"conflicts":[]}));
}
fn extra(ports: &Ports<'_>) {
    let preview =
        super::batch_plan::preview(ports, json!({"phase":"docs","additional_careful":true}));
    super::batch_plan::preview(
        ports,
        json!({"phase":"docs","additional_careful":true,"confirm":true,"digest":preview["extra"]["digest"]}),
    );
}
fn output(ports: &Ports<'_>) -> Value {
    let response = call(ports, "office.consolidate", json!({"batch_id":"docs"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["failed"], json!([]), "{response}");
    response["output"].clone()
}
fn filed(ports: &Ports<'_>, id: &str) -> Value {
    let markdown = ports
        .library
        .read_text(std::path::Path::new(&format!("proposals/open/{id}.md")))
        .unwrap();
    serde_json::from_str(
        markdown
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap(),
    )
    .unwrap()
}
fn page(library: &FakeLibrary, title: &str, aliases: &str) {
    library.write_document(std::path::Path::new("knowledge/domain/dom-topic-existing.md"),
        &format!("---\nid: dom-topic-existing\ntitle: {title}\ntemplate: topic\naliases: {aliases}\nrefs: [{{label: Owner, contact: owner@example.com}}]\nsensitivity: normal\n---\n## Overview\nExisting content.\n")).unwrap();
}

#[test]
fn alias_and_overlap_matches_become_updates_with_the_original_revision_and_contacts() {
    for (title, aliases, subjects) in [
        (
            "Pipeline",
            "[INGEST]",
            json!([{"name":"Ingest","template":"topic"},{"name":"PIPELINE","template":"topic"}]),
        ),
        (
            "Ingest service pipeline",
            "[]",
            json!([{"name":"Service pipeline ingest","template":"topic"}]),
        ),
        (
            "Data ingest service pipeline",
            "[]",
            json!([{"name":"Data ingest service","template":"topic"}]),
        ),
    ] {
        let (ports, models, orbit) = configured(vec![digest(subjects); 2], |library| {
            page(library, title, aliases)
        });
        body(&models, "## Overview\nMerged content.");
        proposed(&orbit, "proposal-update");
        let result = output(&ports);
        assert_eq!(result["merged"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(result["proposed"][0]["page"], "dom-topic-existing");
        let payload = filed(&ports, "proposal-update");
        assert_eq!(payload["kind"], "knowledge_update");
        let original = ports
            .library
            .read_text(std::path::Path::new(
                "knowledge/domain/dom-topic-existing.md",
            ))
            .unwrap();
        assert_eq!(
            payload["base_sha256"],
            office_core::domain::source::sha256_hex(original.as_bytes())
        );
        assert!(
            payload["markdown"]
                .as_str()
                .unwrap()
                .contains("owner@example.com")
        );
        let prompt = models.extract_requests.borrow()[0].text.clone();
        assert!(!prompt.contains("owner@example.com"));
        assert!(prompt.contains("Existing content"));
        assert_eq!(models.extract_requests.borrow().len(), 1);
        assert!(
            orbit.calls()[1].input["tags"]
                .as_array()
                .unwrap()
                .contains(&json!("proposal:knowledge_update"))
        );
    }
}

#[test]
fn transitive_aliases_group_independently_of_digest_subject_order_and_count_sources_once() {
    let subjects = json!([
        {"name":"INGEST","aliases":["Pipeline"],"template":"topic"},
        {"name":"PipeLine","aliases":["Data flow"],"template":"topic"},
        {"name":"DATA-FLOW","template":"topic"}
    ]);
    let mut prior = None;
    for reversed in [false, true] {
        let mut value = digest(subjects.clone());
        if reversed {
            value["subjects"].as_array_mut().unwrap().reverse();
        }
        let (ports, models, orbit) = fixture(vec![value; 4]);
        body(&models, "## Overview\nMerged design.");
        proposed(&orbit, "proposal-alias");
        let result = output(&ports);
        assert_eq!(result["proposed"].as_array().unwrap().len(), 1);
        let request = models.extract_requests.borrow()[0].text.clone();
        if let Some(prior) = &prior {
            assert_eq!(&request, prior);
        }
        prior = Some(request);
    }
    let (ports, models, orbit) = fixture(vec![digest(subjects)]);
    let result = output(&ports);
    assert_eq!(result["below_bar"], json!(["data flow"]));
    assert!(models.extract_requests.borrow().is_empty());
    assert!(orbit.calls().is_empty());
}

fn procedure() -> Value {
    json!({"title":"Rotate ingest keys","inputs":["Approved key"],"steps":[{"text":"Prepare the key","cites":["#L2"]},{"text":"Rotate the key","cites":["#L3"]}],"done_when":"The new key works"})
}

#[test]
fn incomplete_procedures_fold_into_topics_and_only_complete_shapes_get_runbooks() {
    for missing in ["inputs", "steps", "done_when", "cites", "dangling"] {
        let mut value = digest(json!([{"name":"Ingest","template":"topic"}]));
        let mut p = procedure();
        match missing {
            "inputs" => p["inputs"] = json!([]),
            "steps" => {
                p["steps"].as_array_mut().unwrap().pop();
            }
            "done_when" => p["done_when"] = json!(""),
            "cites" => p["steps"][0]["cites"] = json!([]),
            "dangling" => p["steps"][0]["cites"] = json!(["#L999"]),
            _ => unreachable!(),
        }
        value["subjects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"Rotate ingest keys","template":"runbook"}));
        value["procedures"] = json!([p]);
        let (ports, models, orbit) = fixture(vec![value; 4]);
        body(
            &models,
            "## Overview\nIngest design.\n\n## How it works here\nProcedure still needs details.",
        );
        proposed(&orbit, "proposal-topic");
        let result = output(&ports);
        assert_eq!(
            result["proposed"].as_array().unwrap().len(),
            1,
            "{missing}: {result}"
        );
        let payload = filed(&ports, "proposal-topic");
        assert!(
            payload["markdown"]
                .as_str()
                .unwrap()
                .contains("template: \"topic\"")
        );
        let prompt: Value =
            serde_json::from_str(&models.extract_requests.borrow()[0].text).unwrap();
        assert_eq!(prompt["procedures"].as_array().unwrap().len(), 4);
        assert!(
            prompt["procedures"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["title"] == "Rotate ingest keys")
        );
    }
    let mut value = digest(json!([{"name":"Ingest","template":"topic"}]));
    value["procedures"] = json!([procedure()]);
    let (ports, models, orbit) = fixture(vec![value; 4]);
    extra(&ports);
    body(&models, "## Overview\nIngest design.");
    body(
        &models,
        "## Inputs\nApproved key.\n## Steps\n1. Prepare [evidence](src-0#L2).\n2. Rotate [evidence](src-0#L3).\n## Done when\nThe key works.",
    );
    proposed(&orbit, "proposal-topic");
    proposed(&orbit, "proposal-runbook");
    let result = output(&ports);
    assert_eq!(result["proposed"].as_array().unwrap().len(), 2, "{result}");
    assert!(
        filed(&ports, "proposal-runbook")["markdown"]
            .as_str()
            .unwrap()
            .contains("template: \"runbook\"")
    );
}

#[test]
fn volatile_and_uncited_claims_never_reach_the_careful_model_and_each_group_calls_once() {
    let mut value = digest(json!([{"name":"Ingest","template":"topic"}]));
    value["claims"] = json!([
        {"subject":"INGEST","text":"Durable claim owner@example.com","cites":["#L2"],"volatile":false},
        {"subject":"Ingest","text":"Volatile sprint secret","cites":["#L2"],"volatile":true},
        {"subject":"Ingest","text":"Uncited invention","cites":[],"volatile":false},
        {"subject":"Ingest","text":"Dangling invention","cites":["#L999"],"volatile":false}
    ]);
    let (ports, models, orbit) = fixture(vec![value; 4]);
    body(&models, "## Overview\nDurable claim [evidence](src-0#L2).");
    proposed(&orbit, "proposal-safe");
    output(&ports);
    let requests = models.extract_requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].tier, office_core::ports::ModelTier::Careful);
    let prompt: Value = serde_json::from_str(&requests[0].text).unwrap();
    assert_eq!(prompt["claims"].as_array().unwrap().len(), 4);
    assert!(!requests[0].text.contains("Volatile sprint secret"));
    assert!(!requests[0].text.contains("owner@example.com"));
    assert!(!requests[0].text.contains("Uncited invention"));
    assert!(!requests[0].text.contains("Dangling invention"));
    assert!(requests[0].text.contains("src-0#L2"));
}

#[test]
fn nine_sources_budget_three_new_pages_but_updates_do_not_count_and_overflow_is_durable() {
    let value = digest(json!([
        {"name":"Alpha","template":"topic"},{"name":"Beta","template":"topic"},
        {"name":"Charlie","template":"topic"},{"name":"Delta","template":"topic"},
        {"name":"Existing","template":"topic"}
    ]));
    let (ports, models, orbit) =
        configured(vec![value; 9], |library| page(library, "Existing", "[]"));
    extra(&ports);
    for id in ["proposal-a", "proposal-b", "proposal-c", "proposal-update"] {
        body(&models, "## Overview\nConsolidated page.");
        proposed(&orbit, id);
    }
    let result = output(&ports);
    assert_eq!(result["new_page_budget"], 3);
    assert_eq!(result["proposed"].as_array().unwrap().len(), 4);
    assert_eq!(result["merged"].as_array().unwrap().len(), 1);
    assert_eq!(result["deferred"], json!(["delta"]));
    assert_eq!(models.extract_requests.borrow().len(), 4);
    assert!(
        models
            .extract_requests
            .borrow()
            .iter()
            .all(|r| r.tier == office_core::ports::ModelTier::Careful)
    );
    let snapshot = ports.library.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.batches["docs"].overflow, ["delta"]);
    assert!(snapshot.batches["docs"].consolidating.is_empty());
    // A carry-over plan can also include a fresh source: publishing its
    // stage-one digest must not revoke approval, while changing a carried
    // digest must revoke it.
    let mut mixed = snapshot.clone();
    mixed.sources.push(bootstrap::Source {
        path: "sources/1970/01/src-fresh/source.md".into(),
        markdown:
            "---\nid: src-fresh\nkind: doc\ntags: [bootstrap:docs]\n---\n# Fresh\nEvidence.\n"
                .into(),
        digest: None,
    });
    let settings = ports.library.settings().unwrap();
    let mixed_plan = bootstrap::plan(&mixed, &settings, "docs", false, vec![]).unwrap();
    mixed.sources.last_mut().unwrap().digest =
        Some(digest(json!([{"name":"Fresh","template":"topic"}])));
    bootstrap::verify_plan(&mixed, &settings, &mixed_plan).unwrap();
    mixed.sources[0].digest.as_mut().unwrap()["summary"] = json!("Changed carried evidence");
    assert_eq!(
        bootstrap::verify_plan(&mixed, &settings, &mixed_plan)
            .unwrap_err()
            .code(),
        "plan_changed"
    );
    assert_eq!(
        call(&ports, "office.consolidate", json!({"batch_id":"docs"}))["error"]["code"],
        "batch_started"
    );
    assert_eq!(models.extract_requests.borrow().len(), 4);
    let next = super::batch_plan::confirm(&ports, "docs");
    assert_eq!(next["plan"]["deferred_subjects"], json!(["delta"]));
    assert_eq!(next["plan"]["counts"]["digest"], 0);
    body(&models, "## Overview\nDeferred page.");
    proposed(&orbit, "proposal-deferred");
    let next = output(&ports);
    assert_eq!(next["proposed"].as_array().unwrap().len(), 1, "{next}");
    assert_eq!(next["proposed"][0]["subject"], "delta");
    assert_eq!(models.extract_requests.borrow().len(), 5);
}

#[test]
fn conflicts_preserve_both_citations_with_newer_current_and_reversed_dates_are_refused() {
    let mut values = vec![digest(json!([{"name":"Ingest","template":"topic"}])); 2];
    for (index, (date, text)) in [("2026-01-01", "Old design"), ("2026-02-01", "New design")]
        .into_iter()
        .enumerate()
    {
        values[index]["as_of"] = json!(date);
        values[index]["claims"] =
            json!([{"subject":"Ingest","text":text,"cites":["#L2"],"volatile":false}]);
    }
    let current = json!({"text":"New design","cites":["src-1#L2"]});
    let old = json!({"text":"Old design","cites":["src-0#L2"]});
    for reversed in [false, true] {
        let (ports, models, orbit) = fixture(values.clone());
        models.extract_replies.borrow_mut().push_back(json!({"body":"## Overview\nDesign has changed.","conflicts":[{"current":if reversed { &old } else { &current },"superseded":if reversed { &current } else { &old }}]}));
        if !reversed {
            proposed(&orbit, "proposal-conflict");
        }
        let result = call(&ports, "office.consolidate", json!({"batch_id":"docs"}));
        if reversed {
            assert_eq!(
                result["output"]["failed"][0]["code"], "model_invalid_response",
                "{result}"
            );
            assert!(orbit.calls().is_empty());
        } else {
            assert_eq!(
                result["output"]["conflicts"][0]["claims"][0]["current"], current,
                "{result}"
            );
            let payload = filed(&ports, "proposal-conflict");
            let markdown = payload["markdown"].as_str().unwrap();
            assert!(markdown.contains("Current (\"2026-02-01\"): New design [src-1#L2]"));
            assert!(markdown.contains("Superseded (\"2026-01-01\"): Old design [src-0#L2]"));
        }
    }
}

#[test]
fn invalid_unconfirmed_missing_digests_and_changed_settings_make_no_model_or_orbit_calls() {
    let (ports, models, orbit) = fixture(vec![
        digest(json!([{"name":"Ingest","template":"topic"}]));
        2
    ]);
    for input in [
        json!({}),
        json!({"batch_id":"bad"}),
        json!({"batch_id":"docs","force":true}),
        json!({"batch_id":"docs","requested":[1]}),
    ] {
        let before = ports.library.head_commit().unwrap();
        assert_eq!(
            call(&ports, "office.consolidate", input)["error"]["code"],
            "invalid_argument"
        );
        assert_eq!(ports.library.head_commit().unwrap(), before);
    }
    assert_eq!(
        call(&ports, "office.consolidate", json!({"batch_id":"kt"}))["error"]["code"],
        "plan_unconfirmed"
    );
    ports
        .library
        .set_setting("models.careful", json!("configured-careful"))
        .unwrap();
    assert_eq!(
        call(&ports, "office.consolidate", json!({"batch_id":"docs"}))["error"]["code"],
        "plan_changed"
    );
    assert!(models.extract_requests.borrow().is_empty());
    assert!(orbit.calls().is_empty());
    let ports = super::batch_plan::fixture();
    super::batch_plan::confirm(&ports, "docs");
    assert_eq!(
        call(&ports, "office.consolidate", json!({"batch_id":"docs"}))["error"]["code"],
        "digests_incomplete"
    );
}

#[test]
fn additional_calls_need_confirmation_and_failures_consume_slots_without_retrying_or_stopping_other_groups()
 {
    let value =
        digest(json!([{"name":"Alpha","template":"topic"},{"name":"Beta","template":"topic"}]));
    let (mut ports, models, orbit) = fixture(vec![value; 4]);
    let before = ports.library.head_commit().unwrap();
    assert_eq!(
        call(&ports, "office.consolidate", json!({"batch_id":"docs"}))["error"]["code"],
        "plan_exceeded"
    );
    assert_eq!(ports.library.head_commit().unwrap(), before);
    assert!(models.extract_requests.borrow().is_empty());
    extra(&ports);
    let mut failing = models.clone();
    failing.extract_failure = Some(0);
    ports.models = Box::new(failing);
    body(&models, "## Overview\nFirst.");
    body(&models, "## Overview\nSecond.");
    proposed(&orbit, "proposal-second");
    let response = call(&ports, "office.consolidate", json!({"batch_id":"docs"}));
    assert_eq!(
        response["output"]["failed"],
        json!([{"subject":"alpha","code":"model_unavailable"}]),
        "{response}"
    );
    assert_eq!(response["output"]["proposed"].as_array().unwrap().len(), 1);
    let snapshot = ports.library.bootstrap_snapshot().unwrap();
    let batch = &snapshot.batches["docs"];
    assert_eq!(batch.used.iter().sum::<usize>() + batch.extra_used, 2);
    assert_eq!(batch.consolidating, ["alpha"]);
    assert_eq!(models.extract_requests.borrow().len(), 2);
}

#[test]
fn explicit_request_and_singleton_template_bypass_the_source_bar() {
    for (name, template, requested, body_text) in [
        (
            "Ingest",
            "topic",
            json!(["INGEST"]),
            "## Overview\nRequested knowledge.",
        ),
        (
            "Glossary",
            "glossary",
            json!([]),
            "## Terms\nDLQ means dead letter queue.",
        ),
    ] {
        let (ports, models, orbit) =
            fixture(vec![digest(json!([{"name":name,"template":template}]))]);
        body(&models, body_text);
        proposed(&orbit, "proposal-requested");
        let result = call(
            &ports,
            "office.consolidate",
            json!({"batch_id":"docs","requested":requested}),
        );
        assert_eq!(result["output"]["failed"], json!([]), "{result}");
        assert_eq!(
            result["output"]["proposed"].as_array().unwrap().len(),
            1,
            "{result}"
        );
        assert_eq!(models.extract_requests.borrow().len(), 1);
    }
}

#[test]
fn conformance_golden_replays_success_and_refusal() {
    let (ports, models, orbit) =
        fixture(vec![digest(json!([{"name":"Ingest","template":"topic"}]))]);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/consolidate.yaml");
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
                let response = call(&ports, "office.consolidate", input.take().unwrap());
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
    assert!(models.extract_requests.borrow().is_empty());
    assert!(orbit.calls().is_empty());
}

#[test]
fn mixed_private_evidence_keeps_every_careful_call_private() {
    use office_core::domain::endpoint::Capability;
    let (ports, models, orbit) = configured(
        vec![digest(json!([{"name":"Ingest","template":"topic"}])); 4],
        |library| {
            library.set_setting("open_weights",json!({"endpoint":"https://private.example.com/v1","model":"private-model","kind":"company","label":"Company"})).unwrap();
            library
                .record_route_check(
                    "https://private.example.com/v1",
                    "private-model",
                    &[Capability::Text],
                    "1970-01-01T00:00:00Z",
                )
                .unwrap();
            let path = std::path::Path::new("sources/1970/01/src-0/source.md");
            let text = library
                .read_text(path)
                .unwrap()
                .replace("sensitivity: normal", "sensitivity: private");
            library.write_document(path, &text).unwrap();
        },
    );
    body(&models, "## Overview\nMerged private evidence.");
    proposed(&orbit, "proposal-private");
    output(&ports);
    let request = &models.extract_requests.borrow()[0];
    assert_eq!(request.tier, office_core::ports::ModelTier::Careful);
    assert_eq!(
        request.sensitivity,
        office_core::ports::Sensitivity::Private
    );
    assert!(
        filed(&ports, "proposal-private")["markdown"]
            .as_str()
            .unwrap()
            .contains("sensitivity: \"private\"")
    );
    assert_eq!(
        ports.library.bootstrap_snapshot().unwrap().batches["docs"]
            .plan
            .groups
            .last()
            .unwrap()
            .route
            .model,
        "private-model"
    );
}
