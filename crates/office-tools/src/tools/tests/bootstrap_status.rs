//! Setup status through public dispatch.
use super::{batch_plan::fixture, video_plan::call};
use serde_json::json;

#[test]
fn reports_all_four_phases_without_writing() {
    let ports = fixture();
    let head = ports.library.head_commit().unwrap();
    let response = call(&ports, "office.bootstrap-status", json!({}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["phases"].as_array().unwrap().len(), 4);
    assert_eq!(response["output"]["phases"][0]["captured"], 1);
    assert_eq!(response["output"]["phases"][0]["digested"], 0);
    assert_eq!(ports.library.head_commit().unwrap(), head);
}

fn status_ports(
    library: office_testkit::fakes::FakeLibrary,
    tasks: serde_json::Value,
) -> office_core::ports::Ports<'static> {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(
        json!({"total":tasks.as_array().unwrap().len(),"truncated":false,"tasks":tasks}),
    ));
    let mut ports = super::batch_plan::ports(library);
    ports.orbit = Box::new(orbit);
    ports
}

#[test]
fn exact_gap_fixture_counts_progress_and_proposal_decisions_without_extra_gaps() {
    use super::batch_plan::{digest, source};
    use office_core::{
        domain::bootstrap::{self, Mutation},
        ports::Library,
    };
    use office_testkit::fakes::FakeLibrary;
    use std::path::Path;
    let library = FakeLibrary::initialized();
    for id in ["one", "two", "three"] {
        source(&library, id, "process", "# Ingest");
    }
    // The consolidation runner records active groups and page-cap overflow in the ledger.
    let snapshot = library.bootstrap_snapshot().unwrap();
    let settings = library.settings().unwrap();
    let plan = bootstrap::plan(&snapshot, &settings, "process", false, vec![]).unwrap();
    let at = "1970-01-01T00:00:00Z".to_string();
    library
        .mutate_batch(
            &snapshot.revision(&settings).unwrap(),
            Mutation::Plan {
                plan: Box::new(plan.clone()),
                confirm: None,
                at: at.clone(),
            },
        )
        .unwrap();
    let revision = library
        .bootstrap_snapshot()
        .unwrap()
        .revision(&settings)
        .unwrap();
    library
        .mutate_batch(
            &revision,
            Mutation::Plan {
                plan: Box::new(plan.clone()),
                confirm: Some(plan.digest),
                at,
            },
        )
        .unwrap();
    let revision = library
        .bootstrap_snapshot()
        .unwrap()
        .revision(&settings)
        .unwrap();
    library
        .mutate_batch(
            &revision,
            Mutation::Progress {
                phase: "process".into(),
                consolidating: vec!["ingest".into()],
                overflow: vec!["later".into(), "later".into(), "other".into()],
            },
        )
        .unwrap();
    for id in ["one", "two", "three"] {
        let mut value = digest(json!([
            {"name":"Ingest","template":"topic","aliases":["Pipeline"]},
            {"name":"Covered subject","template":"topic"},
            {"name":"Alex","template":"person","role":"owner"}
        ]));
        if id == "one" {
            value["subjects"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name":"Priya Raman","template":"person","role":"ingest owner"}));
            value["procedures"] = json!([{"title":"Rotate keys"},{"title":"Existing procedure"}]);
        }
        library.insert_digest(id, value).unwrap();
    }
    for (id, fields) in [
        (
            "dom-covered",
            "title: Covered\naliases: [Covered subject]\ntemplate: topic",
        ),
        ("dom-alex", "title: Alex\ntemplate: person"),
        (
            "rb-existing",
            "title: Existing procedure\ntemplate: runbook",
        ),
        (
            "dom-policy",
            "title: PTO policy\ntemplate: topic\ntags: [policy]\ncites: [one#L1]",
        ),
        (
            "dom-current-policy",
            "title: Current policy\ntemplate: topic\ntags: [policy]\nas_of: 1970-01-01\ncites: [one#L1]",
        ),
    ] {
        let folder = if id.starts_with("rb-") {
            "runbooks"
        } else {
            "domain"
        };
        library
            .write_document(
                Path::new(&format!("knowledge/{folder}/{id}.md")),
                &format!("---\nid: {id}\n{fields}\n---\nMaintained page.\n"),
            )
            .unwrap();
    }
    let tasks = json!([
        {"id":"pending","status":"proposed","tags":["proposal","proposal:knowledge_add","bootstrap:process"]},
        {"id":"accepted","status":"done","tags":["proposal","proposal:knowledge_update","bootstrap:process"]},
        {"id":"dismissed","status":"rejected","tags":["proposal","proposal:knowledge_add","bootstrap:process"]},
        {"id":"unrelated","status":"proposed","tags":["proposal","proposal:routine","bootstrap:process"]}
    ]);
    let ports = status_ports(library, tasks);
    let before = ports.library.head_commit().unwrap();
    let response = call(&ports, "office.bootstrap-status", json!({}));
    assert_eq!(response["ok"], true, "{response}");
    let phase = &response["output"]["phases"][3];
    for (field, expected) in [
        ("captured", 3),
        ("digested", 3),
        ("consolidating", 1),
        ("proposed", 1),
        ("accepted", 1),
        ("dismissed", 1),
        ("overflow_waiting", 2),
    ] {
        assert_eq!(phase[field], expected, "{field}: {phase}");
    }
    let mut gaps = phase["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g["kind"].as_str().unwrap(), g["subject"].as_str().unwrap()))
        .collect::<Vec<_>>();
    gaps.sort();
    assert_eq!(
        gaps,
        vec![
            ("owner_without_person", "priya raman"),
            ("policy_without_current_as_of", "pto policy"),
            ("procedure_without_runbook", "rotate keys"),
            ("subject_without_page", "ingest")
        ]
    );
    assert_eq!(phase["gaps"][3]["sources"], json!(["one", "three", "two"]));
    for phase in response["output"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .take(3)
    {
        assert_eq!(phase["captured"], 0);
        assert_eq!(phase["gaps"], json!([]));
    }
    assert_eq!(ports.library.head_commit().unwrap(), before);
}

#[test]
fn status_pending_batches_list_only_confirmed_unconsolidated_batches() {
    use office_core::{
        domain::bootstrap::{self, Mutation},
        ports::Library,
    };
    use office_testkit::fakes::FakeLibrary;
    let empty = call(&fixture(), "office.bootstrap-status", json!({}));
    assert_eq!(empty["output"]["pending_batches"], json!([]));
    let library = FakeLibrary::initialized();
    super::batch_plan::source(&library, "one", "docs", "# Ingest");
    super::batch_plan::source(&library, "two", "kt", "# Ingest");
    super::batch_plan::source(&library, "three", "team", "# Ingest");
    let settings = library.settings().unwrap();
    let at = "1970-01-01T00:00:00Z".to_string();
    // docs: confirmed and pending. kt: confirmed but consolidation recorded.
    // team: preview only, never confirmed.
    for (phase, confirm_batch) in [("docs", true), ("kt", true), ("team", false)] {
        let snapshot = library.bootstrap_snapshot().unwrap();
        let plan = bootstrap::plan(&snapshot, &settings, phase, false, vec![]).unwrap();
        let digest = plan.digest.clone();
        library
            .mutate_batch(
                &snapshot.revision(&settings).unwrap(),
                Mutation::Plan {
                    plan: Box::new(plan),
                    confirm: None,
                    at: at.clone(),
                },
            )
            .unwrap();
        if confirm_batch {
            let revision = library
                .bootstrap_snapshot()
                .unwrap()
                .revision(&settings)
                .unwrap();
            // Recompute the stored preview's plan for the confirmation call.
            let snapshot = library.bootstrap_snapshot().unwrap();
            let plan = bootstrap::plan(&snapshot, &settings, phase, false, vec![]).unwrap();
            library
                .mutate_batch(
                    &revision,
                    Mutation::Plan {
                        plan: Box::new(plan),
                        confirm: Some(digest),
                        at: at.clone(),
                    },
                )
                .unwrap();
        }
    }
    let revision = library
        .bootstrap_snapshot()
        .unwrap()
        .revision(&settings)
        .unwrap();
    library
        .mutate_batch(
            &revision,
            Mutation::Progress {
                phase: "kt".into(),
                consolidating: vec!["ingest".into()],
                overflow: vec![],
            },
        )
        .unwrap();
    let ports = status_ports(library, json!([]));
    let response = call(&ports, "office.bootstrap-status", json!({}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        response["output"]["pending_batches"],
        json!([{"batch_id":"docs","phase":"docs"}])
    );
}

#[test]
fn status_refuses_incomplete_task_reads_unknown_input_and_invalid_digests() {
    let ports = fixture();
    assert_eq!(
        call(&ports, "office.bootstrap-status", json!({"phase":"docs"}))["error"]["code"],
        "invalid_argument"
    );
    for value in [
        json!({"tasks":[],"total":1,"truncated":true}),
        json!({"tasks":[],"total":1,"truncated":false}),
        json!({"tasks":[{"id":"x","tags":[]}],"total":1,"truncated":false}),
    ] {
        let orbit = office_testkit::fakes::FakeOrbit::default();
        orbit.push_response(Ok(value));
        let mut ports = fixture();
        ports.orbit = Box::new(orbit);
        let response = call(&ports, "office.bootstrap-status", json!({}));
        assert_eq!(response["ok"], false, "{response}");
    }
    let library = office_testkit::fakes::FakeLibrary::initialized();
    super::batch_plan::source(&library, "one", "docs", "Material");
    assert!(
        library
            .insert_digest("one", json!({"subjects":[]}))
            .is_err()
    );
}

#[test]
fn repeated_mentions_in_one_source_are_not_three_sources_and_aliases_cover_owners() {
    use office_core::ports::Library;
    use std::path::Path;
    let library = office_testkit::fakes::FakeLibrary::initialized();
    super::batch_plan::source(&library, "one", "docs", "Material");
    library.insert_digest("one",super::batch_plan::digest(json!([
        {"name":"Only one","template":"topic"},{"name":"Only one","template":"topic"},{"name":"Only one","template":"topic"},
        {"name":"Priya Raman","template":"person","role":"owner","aliases":["Priya"]}
    ]))).unwrap();
    library
        .write_document(
            Path::new("knowledge/domain/dom-priya.md"),
            "---\nid: dom-priya\ntitle: Priya\ntemplate: person\n---\nWork contact.",
        )
        .unwrap();
    let response = call(
        &status_ports(library, json!([])),
        "office.bootstrap-status",
        json!({}),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["phases"][0]["gaps"], json!([]));
}

#[test]
fn bootstrap_conformance_goldens_replay_offline_success_and_refusal() {
    use std::path::Path;
    for verb in ["batch-plan", "bootstrap-status"] {
        let ports = fixture();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(&path).unwrap();
        let mut input = None;
        let mut updated = Vec::new();
        let (mut successes, mut refusals) = (0, 0);
        for line in golden.lines() {
            if let Some(raw) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<serde_json::Value>(raw).unwrap());
            }
            let mut replacement = None;
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(raw) = line.strip_prefix(prefix) {
                    let expected: serde_json::Value = serde_json::from_str(raw).unwrap();
                    let response = call(&ports, &format!("office.{verb}"), input.take().unwrap());
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        successes += 1;
                        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
                            replacement = Some(format!("{prefix}{}", response["output"]));
                        } else {
                            assert_eq!(response["output"], expected);
                        }
                    } else {
                        refusals += 1;
                        assert_eq!(response["error"]["code"], expected["code"]);
                    }
                }
            }
            updated.push(replacement.unwrap_or_else(|| line.into()));
        }
        assert!(successes > 0 && refusals > 0);
        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
            std::fs::write(path, format!("{}\n", updated.join("\n"))).unwrap();
        }
    }
}

#[test]
fn policy_as_of_must_be_current_and_no_older_than_cited_evidence() {
    use office_core::ports::Library;
    use std::path::Path;
    let library = office_testkit::fakes::FakeLibrary::initialized();
    super::batch_plan::source(&library, "one", "process", "Material.");
    let mut value = super::batch_plan::digest(json!([]));
    value["as_of"] = json!("1970-01-01");
    library.insert_digest("one", value).unwrap();
    for (name, as_of) in [
        ("old", "1969-12-31"),
        ("future", "1970-01-02"),
        ("current", "1970-01-01"),
        ("invalid", "not-a-date"),
    ] {
        library.write_document(Path::new(&format!("knowledge/domain/dom-{name}.md")),&format!("---\nid: dom-{name}\ntitle: {name}\ntags: [policy]\nas_of: {as_of}\ncites: [one#L1]\n---\nPolicy summary.\n")).unwrap();
    }
    let response = call(
        &status_ports(library, json!([])),
        "office.bootstrap-status",
        json!({}),
    );
    assert_eq!(response["ok"], true, "{response}");
    let names = response["output"]["phases"][3]["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["subject"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["future", "invalid", "old"]);
}
