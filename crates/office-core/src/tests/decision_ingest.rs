//! Orbit decision ingestion planning: selection, mapping, feedback links,
//! dedupe and refusals (decision-precedents.md, Ingesting Orbit decisions).

use crate::domain::source::decision_ingest::{Listed, plan, scope_of, workspace_of};
use crate::domain::source::decisions::{Ledger, Scope};
use serde_json::{Value, json};

fn request(id: &str, gate: &str, answer: &str, by: &str, at: &str) -> Value {
    json!({
        "id": id, "task_id": "ORB-1", "gate": gate, "state": "answered",
        "answers": [{"id": "1", "label": "Requeue"}, {"id": "confirm", "label": "Confirm"},
                    {"id": "veto", "label": "Veto"}],
        "resolution": {"answer_id": answer, "answered_by": by, "answered_at": at}
    })
}

fn listed(requests: Vec<Value>) -> Vec<Listed> {
    requests
        .into_iter()
        .map(|request| Listed {
            selector: None,
            request,
        })
        .collect()
}

fn run(requests: Vec<Value>) -> crate::domain::source::decision_ingest::Plan {
    plan(
        &[],
        &listed(requests),
        None,
        "2026-11-01T00:00:00Z",
        |_, _| None,
    )
    .expect("plan")
}

fn reasons(plan: &crate::domain::source::decision_ingest::Plan) -> Vec<(String, String)> {
    plan.skipped
        .iter()
        .map(|s| {
            (
                s["request_id"].as_str().unwrap_or_default().into(),
                s["reason"].as_str().unwrap_or_default().into(),
            )
        })
        .collect()
}

#[test]
fn workspaces_map_to_project_scopes_and_everything_else_is_global() {
    assert_eq!(
        workspace_of(Some("ws_orbit-office/agent_request"), None).as_deref(),
        Some("ws_orbit-office")
    );
    assert_eq!(
        workspace_of(None, Some("hm_example/ws_acme")).as_deref(),
        Some("ws_acme")
    );
    assert_eq!(workspace_of(None, Some("/home/u/workspaces/x")), None);
    assert_eq!(
        scope_of(Some("ws_orbit-office")),
        Scope::Project("orbit-office".into())
    );
    assert_eq!(scope_of(Some("ws_Not_A_Slug")), Scope::Global);
    assert_eq!(scope_of(Some("ws_global")), Scope::Global);
    assert_eq!(scope_of(None), Scope::Global);
}

#[test]
fn a_review_without_its_model_entry_an_unknown_answer_and_a_missing_resolution_are_skipped() {
    let mut review = request(
        "dr-review",
        "model_answer_review",
        "confirm",
        "daniel",
        "2026-10-28T10:00:00Z",
    );
    review["binding"] = json!({"request_id": "dr-never-ingested"});
    let unknown = request(
        "dr-unknown",
        "block_triage",
        "9",
        "daniel",
        "2026-10-28T10:00:00Z",
    );
    let mut missing = request(
        "dr-missing",
        "block_triage",
        "1",
        "daniel",
        "2026-10-28T10:00:00Z",
    );
    missing["resolution"] = Value::Null;
    let plan = run(vec![review, unknown, missing, json!({"no": "id"})]);
    assert!(plan.files.is_empty());
    assert_eq!(
        reasons(&plan),
        vec![
            ("dr-missing".into(), "resolution_missing".into()),
            ("<unknown>".into(), "unreadable".into()),
            ("dr-review".into(), "review_target_missing".into()),
            ("dr-unknown".into(), "answer_unknown".into()),
        ]
    );
}

#[test]
fn an_entry_failing_ledger_validation_is_refused_whole_and_others_still_append() {
    let mut bad = request(
        "dr-bad",
        "block_triage",
        "1",
        "model",
        "2026-10-28T10:00:00Z",
    );
    bad["resolution"]["model"] = json!({"probability": 3});
    let good = request(
        "dr-good",
        "block_triage",
        "1",
        "daniel",
        "2026-10-28T11:00:00Z",
    );
    let plan = run(vec![bad, good]);
    assert_eq!(plan.appended.len(), 1);
    assert_eq!(plan.appended[0]["request_id"], "dr-good");
    assert_eq!(plan.appended[0]["entry"], "dec-global-1");
    assert_eq!(plan.skipped[0]["reason"], "invalid");
    assert_eq!(plan.skipped[0]["code"], "invalid_decision");
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].expected, None, "a new ledger");
    let ledger = Ledger::parse(&plan.files[0].path, &plan.files[0].text).expect("ledger");
    assert_eq!(ledger.entries.len(), 1);
}

#[test]
fn a_model_entry_planned_in_the_same_run_is_the_review_target_and_a_veto_by_the_model_links_nothing()
 {
    let model = request("dr-m", "block_triage", "1", "model", "2026-10-28T10:00:00Z");
    let mut confirm = request(
        "dr-c",
        "model_answer_review",
        "confirm",
        "daniel",
        "2026-10-28T11:00:00Z",
    );
    confirm["binding"] = json!({"request_id": "dr-m"});
    let mut by_model = request(
        "dr-v",
        "model_answer_review",
        "veto",
        "model",
        "2026-10-28T12:00:00Z",
    );
    by_model["binding"] = json!({"request_id": "dr-m"});
    // Listed newest first; planned oldest first.
    let plan = run(vec![by_model, confirm, model]);
    let ids: Vec<&str> = plan
        .appended
        .iter()
        .map(|a| a["request_id"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids, ["dr-m", "dr-c", "dr-v"]);
    assert_eq!(plan.appended[1]["confirms"], "dec-global-1");
    assert_eq!(plan.appended[2]["reverses"], Value::Null);
    assert_eq!(plan.appended[2]["confirms"], Value::Null);
}

#[test]
fn since_must_be_a_time() {
    let refused = plan(
        &[],
        &[],
        Some("yesterday"),
        "2026-11-01T00:00:00Z",
        |_, _| None,
    );
    assert!(refused.is_err());
}
