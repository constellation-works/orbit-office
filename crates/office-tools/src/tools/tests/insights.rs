//! Analytics through the public exec dispatch, with recorded Orbit and Library facts.

use office_core::domain::settings::Settings;
use office_core::ports::Ports;
use office_testkit::fakes::{FakeLibrary, FakeOrbit, PortsBuilder};
use serde_json::{Value, json};

fn serve(verb: &str, input: Value, ports: &Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(&format!("office.{verb}"), input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request");
    serde_json::from_str(&crate::serve(&request, ports)).expect("response")
}

/// The Library a golden case runs on: policy defaults unless the case names settings.
fn case_library(settings: Option<Value>) -> FakeLibrary {
    let mut library = FakeLibrary::initialized();
    if let Some(settings) = settings {
        library.settings = Settings::try_from(settings).expect("golden settings");
    }
    library
}

fn fixture(tasks: Value, library: FakeLibrary) -> Ports<'static> {
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks": tasks, "total": tasks.as_array().expect("tasks").len(), "truncated": false})));
    let mut ports = PortsBuilder::default().build();
    ports.orbit = Box::new(orbit);
    ports.library = Box::new(library);
    ports
}

#[test]
fn insights_empty_window_is_zero_and_has_no_invented_material() {
    for verb in ["metrics", "review-inputs"] {
        let response = serve(
            verb,
            json!({"since":"2026-10-05","until":"2026-10-19"}),
            &fixture(json!([]), FakeLibrary::initialized()),
        );
        assert_eq!(response["ok"], true, "{response}");
        let report = if verb == "metrics" {
            &response["output"]
        } else {
            &response["output"]["metrics"]
        };
        assert_eq!(report["totals"]["automation"]["total_count"], 0);
        assert_eq!(report["totals"]["automation"]["count_ratio"], Value::Null);
        assert_eq!(report["totals"]["hours_returned"], 0.0);
        assert_eq!(report["weeks"].as_array().expect("weeks").len(), 2);
        if verb == "review-inputs" {
            for key in [
                "completed_tasks",
                "accepted_proposals",
                "conversation_sources",
                "daily_digests",
                "knowledge_changes",
                "outputs",
            ] {
                assert_eq!(response["output"][key], json!([]), "{key}");
            }
        }
    }
}

#[test]
fn insights_refuse_unknown_keys_and_invalid_windows() {
    for verb in ["metrics", "review-inputs"] {
        for input in [
            json!({"extra":true}),
            json!({}),
            json!({"since":"2026-02-30","until":"2026-03-01"}),
            json!({"since":"2026-10-19","until":"2026-10-05"}),
        ] {
            let response = serve(verb, input, &PortsBuilder::default().build());
            assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
        }
    }
}

fn task(id: &str, tags: &[&str], at: Option<&str>, status: &str) -> Value {
    json!({"id":id,"title":id,"type":"chore","status":status,"tags":tags,
        "created_at":"2026-09-28T00:00:00Z",
        "history": at.map(|at| vec![json!({"at":at,"from_status":"backlog","to_status":status})]).unwrap_or_default(),
        "artifacts":[]})
}

fn artifact(
    library: &FakeLibrary,
    id: &str,
    task: &str,
    series: &str,
    at: &str,
    approval: (&str, bool),
    tags: &[&str],
) {
    use office_core::{domain::artifact::Artifact, ports::Library};
    let (by, edited) = approval;
    let value = json!({"id":id,"kind":"report","title":id,"summary":"Recorded output","files":["report.md"],"task":task,
        "produced_by":{"crew":"fixture","runbook":null,"at":at},"approved":{"by":by,"at":at,"edited":edited},
        "cites":[],"series":series,"version":1,"supersedes":null,"sensitivity":"normal","tags":tags});
    let artifact: Artifact = serde_json::from_value(value).expect("artifact");
    library
        .write_document(
            std::path::Path::new(&artifact.path().expect("path")),
            &artifact.to_markdown().expect("markdown"),
        )
        .expect("seed artifact");
}

fn history(digests: bool) -> Ports<'static> {
    let library = FakeLibrary::initialized();
    artifact(
        &library,
        "art-20261006-first",
        "agent-covered",
        "first",
        "2026-10-06T10:00:00Z",
        ("user", false),
        &[],
    );
    artifact(
        &library,
        "art-20261014-edited",
        "agent-bare",
        "edited",
        "2026-10-14T10:00:00Z",
        ("user", true),
        &[],
    );
    artifact(
        &library,
        "art-20261014-routine",
        "routine-run",
        "routine-report",
        "2026-10-14T12:00:00Z",
        ("user", false),
        &["routine:weekly"],
    );
    if digests {
        artifact(
            &library,
            "art-20261007-digest",
            "digest-run",
            "digest-daily",
            "2026-10-07T10:00:00Z",
            ("routine", false),
            &["routine:daily-digest", "estimate-minutes:1000000"],
        );
    }
    let mut covered = task(
        "agent-covered",
        &["owner:agent", "estimate-minutes:60"],
        Some("2026-10-05T00:00:00Z"),
        "done",
    );
    covered["artifacts"] =
        json!([{"path":"context/knowledge/runbooks/rb-report.md"}, {"path":"report.md"}]);
    let tasks = json!([
        task(
            "manual-prior",
            &["owner:you", "cluster:weekly"],
            Some("2026-10-01T12:00:00Z"),
            "done"
        ),
        task(
            "manual-period",
            &["owner:you", "cluster:weekly", "estimate-minutes:30"],
            Some("2026-10-06T12:00:00Z"),
            "done"
        ),
        covered,
        task(
            "agent-bare",
            &["owner:agent"],
            Some("2026-10-13T00:00:00Z"),
            "done"
        ),
        task(
            "proposal-automation",
            &["proposal", "proposal:routine", "cluster:weekly"],
            Some("2026-10-10T10:00:00Z"),
            "done"
        ),
        task(
            "proposal-knowledge",
            &["proposal", "proposal:knowledge_add"],
            Some("2026-10-07T10:00:00Z"),
            "done"
        ),
        task(
            "proposal-dismissed",
            &["proposal", "proposal:auto_task"],
            Some("2026-10-12T10:00:00Z"),
            "rejected"
        ),
        // Current state is done, but this was still open at the period end.
        task(
            "open-manual",
            &["owner:you"],
            Some("2026-10-20T00:00:00Z"),
            "done"
        ),
    ]);
    fixture(tasks, library)
}

fn report(ports: &Ports<'_>) -> Value {
    let response = serve(
        "metrics",
        json!({"since":"2026-10-05","until":"2026-10-19"}),
        ports,
    );
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

#[test]
fn insights_fixture_history_matches_every_hand_calculated_metric() {
    let output = report(&history(false));
    let totals = &output["totals"];
    assert_eq!(
        totals["automation"],
        json!({"automated_count":3,"manual_count":1,"total_count":4,"automated_minutes":105,"manual_minutes":30,"total_minutes":135,"count_ratio":0.75,"minutes_ratio":105.0/135.0})
    );
    assert_eq!(totals["hours_returned"], 1.75);
    assert_eq!(
        totals["first_pass_acceptance"],
        json!({"numerator":2,"denominator":3,"ratio":2.0/3.0})
    );
    assert_eq!(
        totals["knowledge_coverage"],
        json!({"numerator":1,"denominator":2,"ratio":0.5})
    );
    assert_eq!(
        totals["proposal_acceptance"],
        json!({"accepted":2,"dismissed":1,"ratio":2.0/3.0})
    );
    assert_eq!(
        totals["conversions"],
        json!([{"cluster":"weekly","proposal":"proposal-automation","manual_tasks":["manual-period","manual-prior"]}])
    );
    assert_eq!(
        totals["open_load"],
        json!([{"id":"open-manual","title":"open-manual","age_days":21}])
    );
    assert_eq!(
        output["weeks"][0]["metrics"]["automation"]["total_count"],
        2
    );
    assert_eq!(
        output["weeks"][0]["metrics"]["automation"]["automated_minutes"],
        60
    );
    assert_eq!(
        output["weeks"][1]["metrics"]["automation"]["total_count"],
        2
    );
    assert_eq!(
        output["weeks"][1]["metrics"]["proposal_acceptance"]["dismissed"],
        1
    );
}

#[test]
fn insights_daily_digests_change_neither_automation_nor_user_acceptance() {
    let without = report(&history(false));
    let with = report(&history(true));
    assert_eq!(
        with, without,
        "routine-approved housekeeping has no metric effect"
    );
}

#[test]
fn insights_review_selects_tasks_outputs_digests_entries_and_revisions_for_exact_period() {
    use office_core::{domain::knowledge::Revision, ports::Library};
    use std::path::Path;
    let ports = history(true);
    let source_path = "sources/2026/09/src-20260901-session/source.md";
    ports.library.write_document(Path::new(source_path), "---\nid: src-20260901-session\nkind: conversation\ntitle: Session\ncaptured_at: 2026-09-01T00:00:00Z\n---\n[2026-10-04T23:59:59Z] decision: Before\n[2026-10-05T00:00:00Z] decision: Include\n[2026-10-19T00:00:00Z] decision: After\n").expect("source");
    // The fake history helper is exercised separately below; a page update is
    // selected by git revision time even if the current header is newer.
    let library = FakeLibrary::initialized();
    let page_path = "knowledge/domain/dom-project.md";
    library.write_document(Path::new(page_path), "---\nid: dom-project\ntitle: Project\nupdated_at: 2026-11-01T00:00:00Z\n---\nCurrent page").expect("page");
    let revisions: Vec<_> = [
        "2026-10-19T00:00:00Z",
        "2026-10-12T02:00:00+02:00",
        "2026-10-04T23:59:59Z",
    ]
    .iter()
    .enumerate()
    .map(|(i, at)| Revision {
        commit: format!("commit-{i}"),
        author: "Fixture".into(),
        email: "fixture@example.invalid".into(),
        authored_at: (*at).into(),
        message: "Recorded update".into(),
    })
    .collect();
    library
        .set_history(Path::new(page_path), revisions)
        .expect("history");
    // Reuse every seeded artifact and source through the public Library port.
    for file in ports
        .library
        .artifact_documents()
        .expect("artifacts")
        .into_iter()
        .chain(ports.library.source_documents().expect("sources"))
    {
        library
            .write_document(Path::new(&file.path), &file.markdown.expect("markdown"))
            .expect("copy fixture");
    }
    let ports = Ports {
        library: Box::new(library),
        ..ports
    };
    let response = serve(
        "review-inputs",
        json!({"since":"2026-10-05","until":"2026-10-19"}),
        &ports,
    );
    assert_eq!(response["ok"], true, "{response}");
    let output = &response["output"];
    assert_eq!(
        output["completed_tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .map(|v| v["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        ["agent-bare", "agent-covered", "manual-period"]
    );
    assert_eq!(
        output["completed_tasks"][1]["outputs"],
        json!(["context/knowledge/runbooks/rb-report.md", "report.md"])
    );
    assert_eq!(
        output["accepted_proposals"]
            .as_array()
            .expect("proposals")
            .len(),
        2
    );
    assert_eq!(output["outputs"].as_array().expect("outputs").len(), 3);
    assert_eq!(
        output["daily_digests"][0]["artifact"]["series"],
        "digest-daily"
    );
    assert_eq!(
        output["conversation_sources"],
        json!([{"id":"src-20260901-session","title":"Session","path":source_path,"sensitivity":"normal","entries":[{"at":"2026-10-05T00:00:00Z","kind":"decision","text":"Include","cite":"src-20260901-session#L2"}]}])
    );
    assert_eq!(
        output["knowledge_changes"],
        json!([{"id":"dom-project","title":"Project","path":page_path,"revisions":[{"commit":"commit-1","at":"2026-10-12T02:00:00+02:00","message":"Recorded update"}]}])
    );
}

#[test]
fn insights_use_utc_edges_clip_weeks_and_ignore_updated_at() {
    let tasks = json!([
        task(
            "included",
            &["owner:agent"],
            Some("2026-10-04T20:00:00-04:00"),
            "done"
        ),
        task(
            "excluded",
            &["owner:agent"],
            Some("2026-10-19T02:00:00+02:00"),
            "done"
        ),
        task(
            "fraction",
            &["owner:agent"],
            Some("2026-10-18T23:59:59.999Z"),
            "done"
        )
    ]);
    let output = report(&fixture(tasks, FakeLibrary::initialized()));
    assert_eq!(output["totals"]["automation"]["total_count"], 2);
    let response = serve(
        "metrics",
        json!({"since":"2026-10-06","until":"2026-10-14"}),
        &fixture(json!([]), FakeLibrary::initialized()),
    );
    assert_eq!(response["output"]["weeks"][0]["since"], "2026-10-06");
    assert_eq!(response["output"]["weeks"][0]["until"], "2026-10-12");
    assert_eq!(response["output"]["weeks"][1]["until"], "2026-10-14");
}

#[test]
fn insights_refuse_partial_or_malformed_history_and_propagate_callback_errors() {
    use office_core::Error;
    for (reply, code) in [
        (
            Ok(json!({"tasks":[],"total":1,"truncated":true})),
            "task_list_truncated",
        ),
        (
            Ok(json!({"tasks":[],"total":1,"truncated":false})),
            "metrics_data_invalid",
        ),
        (Ok(json!({"tasks":[]})), "metrics_data_invalid"),
        (Err(Error::OrbitUnavailable), "orbit_unavailable"),
    ] {
        let orbit = FakeOrbit::default();
        orbit.push_response(reply);
        let mut ports = PortsBuilder::default().build();
        ports.orbit = Box::new(orbit);
        let response = serve(
            "metrics",
            json!({"since":"2026-10-05","until":"2026-10-19"}),
            &ports,
        );
        assert_eq!(response["error"]["code"], code, "{response}");
    }
    for malformed in [
        task("missing-completion", &["owner:agent"], None, "done"),
        task(
            "bad-estimate",
            &["owner:agent", "estimate-minutes:abc"],
            None,
            "backlog",
        ),
        task(
            "bad-time",
            &["owner:agent"],
            Some("2026-10-05T24:00:00Z"),
            "done",
        ),
    ] {
        let response = serve(
            "metrics",
            json!({"since":"2026-10-05","until":"2026-10-19"}),
            &fixture(json!([malformed]), FakeLibrary::initialized()),
        );
        assert_eq!(
            response["error"]["code"], "metrics_data_invalid",
            "{response}"
        );
    }
}

#[test]
fn insights_runbook_tags_without_attached_files_do_not_claim_coverage() {
    let output = report(&fixture(
        json!([task(
            "named",
            &["owner:agent", "runbook:rb-example"],
            Some("2026-10-06T00:00:00Z"),
            "done"
        )]),
        FakeLibrary::initialized(),
    ));
    assert_eq!(
        output["totals"]["knowledge_coverage"],
        json!({"numerator":0,"denominator":1,"ratio":0.0})
    );
}

#[test]
fn insights_default_minutes_follow_task_kind_and_routine_versions_count_once() {
    let mut tasks = Vec::new();
    for kind in ["chore", "bug", "feature", "refactor"] {
        let mut record = task(kind, &["owner:agent"], Some("2026-10-06T00:00:00Z"), "done");
        record["type"] = json!(kind);
        tasks.push(record);
    }
    let library = FakeLibrary::initialized();
    for suffix in ["one", "two"] {
        artifact(
            &library,
            &format!("art-20261014-{suffix}"),
            "same-routine-run",
            "routine-report",
            "2026-10-14T00:00:00Z",
            ("user", false),
            &["routine:weekly", "estimate-minutes:45"],
        );
    }
    let output = report(&fixture(json!(tasks), library));
    assert_eq!(output["totals"]["automation"]["automated_minutes"], 180);
    assert_eq!(output["totals"]["automation"]["automated_count"], 5);
}

#[test]
fn insights_conformance_goldens_replay_success_and_refusal_offline() {
    for verb in ["metrics", "review-inputs"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(path).expect("golden");
        let mut input = None;
        let mut successes = 0;
        let mut refusals = 0;
        for line in golden.lines() {
            if let Some(value) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<Value>(value).expect("input"));
            }
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(value) = line.strip_prefix(prefix) {
                    let expected: Value = serde_json::from_str(value).expect("expectation");
                    let response = serve(
                        verb,
                        input.take().expect("case input"),
                        &fixture(json!([]), case_library(None)),
                    );
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        assert_eq!(response["output"], expected);
                        successes += 1;
                    } else {
                        assert_eq!(response["error"]["code"], expected["code"]);
                        refusals += 1;
                    }
                }
            }
        }
        assert!(
            successes > 0 && refusals > 0,
            "each golden must execute success and refusal cases"
        );
    }
}

#[test]
fn metrics_reports_simulated_until_when_configured() {
    let library = case_library(Some(json!({"demo": {"simulated_until": "2026-10-12"}})));
    let response = serve(
        "metrics",
        json!({"since": "2026-10-05", "until": "2026-10-12"}),
        &fixture(json!([]), library),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["simulated_until"], "2026-10-12");
}

#[test]
fn insights_routine_work_counts_in_only_its_first_approval_week() {
    let library = FakeLibrary::initialized();
    artifact(
        &library,
        "art-20261006-routine",
        "one-run",
        "report",
        "2026-10-06T00:00:00Z",
        ("user", false),
        &["routine:weekly"],
    );
    artifact(
        &library,
        "art-20261014-routine",
        "one-run",
        "report",
        "2026-10-14T00:00:00Z",
        ("user", true),
        &["routine:weekly"],
    );
    let output = report(&fixture(json!([]), library));
    assert_eq!(output["totals"]["automation"]["total_count"], 1);
    assert_eq!(
        output["weeks"][0]["metrics"]["automation"]["total_count"],
        1
    );
    assert_eq!(
        output["weeks"][1]["metrics"]["automation"]["total_count"],
        0
    );
}

#[test]
fn insights_a_cluster_is_converted_only_at_its_first_accepted_automation() {
    let tasks = json!([
        task(
            "manual",
            &["owner:you", "cluster:reports"],
            Some("2026-10-01T00:00:00Z"),
            "done"
        ),
        task(
            "first",
            &["proposal", "proposal:auto_task", "cluster:reports"],
            Some("2026-10-02T00:00:00Z"),
            "done"
        ),
        task(
            "replacement",
            &["proposal", "proposal:routine", "cluster:reports"],
            Some("2026-10-06T00:00:00Z"),
            "done"
        )
    ]);
    let output = report(&fixture(tasks, FakeLibrary::initialized()));
    assert_eq!(output["totals"]["conversions"], json!([]));
    assert_eq!(output["totals"]["proposal_acceptance"]["accepted"], 1);
}

#[test]
fn review_inputs_select_daily_digests_by_reported_day_across_midnight() {
    let library = FakeLibrary::initialized();
    artifact(
        &library,
        "art-20261027-yesterday",
        "routine:office-daily-digest/2026-10-26",
        "digest-daily",
        "2026-10-27T00:05:00Z",
        ("routine", false),
        &["day:2026-10-26"],
    );
    let response = serve(
        "review-inputs",
        json!({"since":"2026-10-26","until":"2026-10-27"}),
        &fixture(json!([]), library),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        response["output"]["daily_digests"]
            .as_array()
            .expect("digests")
            .len(),
        1
    );
}
