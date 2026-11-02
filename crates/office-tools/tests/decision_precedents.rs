//! Replay the shipped provider golden through the public exec dispatcher.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::ports::Library;
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels, FakeOrbit, PortsBuilder},
};
use serde_json::{Value, json};

#[test]
fn provider_conformance_is_model_free_and_lists_contradiction_todos_on_success() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/decision-precedents.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let models = FakeModels::new();
                let orbit = FakeOrbit::default();
                let mut ports = PortsBuilder::default().build();
                ports.library = Box::new(FakeLibrary::initialized());
                ports.models = Box::new(models.clone());
                ports.orbit = Box::new(orbit.clone());
                let raw = envelope::request(
                    "office.decision-precedents",
                    input.take().unwrap(),
                    json!({}),
                );
                let request = office_core::protocol::parse(&raw).unwrap();
                if success {
                    orbit.push_response(Ok(json!({
                        "tasks": [],
                        "total": 0,
                        "truncated": false
                    })));
                }
                let response = envelope::response(&office_tools::serve(&request, &ports)).unwrap();
                assert_eq!(response["ok"], success, "{response}");
                assert!(models.extract_requests.borrow().is_empty());
                let calls = orbit.calls();
                if success {
                    assert_eq!(response["output"], expected);
                    assert_eq!(calls.len(), 1, "{calls:?}");
                    assert_eq!(calls[0].tool, "orbit.task.list");
                    assert_eq!(
                        calls[0].input.get("tag"),
                        Some(&json!(["owner:you", "office-scribe-contradictions"]))
                    );
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                    assert!(calls.is_empty(), "{calls:?}");
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 4, "every shipped provider conformance case ran");
}

const AS_OF: &str = "2026-11-01T12:00:00Z";

fn request(k: u64) -> Value {
    json!({
        "contract": "orbit.precedent-provider/1",
        "workspace_id": "ws_acme",
        "request": {
            "id": "request-example",
            "gate": "agent_request",
            "category": "ws_acme/pricing",
            "question": "workspace pricing",
            "options": [{"id": "ship", "label": "ship", "consequence": "Release"}],
            "topic": null,
            "raised_at": AS_OF
        },
        "as_of": AS_OF,
        "k": k
    })
}

fn decision(id: &str, question: &str, chosen: &str, at: &str) -> Value {
    json!({
        "id": id,
        "question": question,
        "question_by": "scribe",
        "options": "unrecoverable",
        "chosen": chosen,
        "quote": chosen,
        "ref": "SCR-example#m-1",
        "because": {"text": "Customers asked once", "ref": "SCR-example#m-1"},
        "scope": "global",
        "context": {
            "task_type": "feature",
            "workspace": "ws_acme",
            "task": null,
            "risk_tier": "reversible"
        },
        "decided_by": "human",
        "decided_at": at,
        "reverses": null,
        "source": "SCR-example"
    })
}

fn ledger(entries: &[Value]) -> String {
    let mut text = "---\nid: decisions-global\nkind: decisions\nscope: global\ncreated_at: 2026-10-01T12:00:00Z\n---\n".to_string();
    for entry in entries {
        text.push_str(&format!(
            "- {} {} #decision {entry}\n",
            entry["id"].as_str().unwrap(),
            entry["decided_at"].as_str().unwrap()
        ));
    }
    text
}

fn conflict_line(id: &str) -> String {
    format!("Unresolved conflict with {id}; the user has not said which stands.")
}

fn context_of(row: &Value) -> &str {
    row["context"].as_str().unwrap()
}

/// Serve one provider call. `listed` is the `orbit.task.list` reply; `None` fails the read.
fn serve(entries: &[Value], scores: &[(&str, f64)], listed: Option<Value>, k: u64) -> Value {
    let mut library = FakeLibrary::initialized();
    Library::write_document(
        &library,
        std::path::Path::new("sources/decisions/global.md"),
        &ledger(entries),
    )
    .unwrap();
    for (id, score) in scores {
        library
            .decision_precedent_scores
            .insert((*id).to_string(), *score);
    }
    let models = FakeModels::new();
    let orbit = FakeOrbit::default();
    orbit.push_response(match listed {
        Some(value) => Ok(value),
        None => Err(office_core::Error::OrbitUnavailable),
    });
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.models = Box::new(models.clone());
    ports.orbit = Box::new(orbit.clone());
    let raw = envelope::request("office.decision-precedents", request(k), json!({}));
    let parsed = office_core::protocol::parse(&raw).unwrap();
    let response = envelope::response(&office_tools::serve(&parsed, &ports)).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    assert!(models.extract_requests.borrow().is_empty());
    let calls = orbit.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        Value::Object(calls[0].input.clone()),
        json!({
            "tag": ["owner:you", "office-scribe-contradictions"],
            "status": ["proposed", "backlog", "in-progress", "review", "blocked", "someday"],
            "limit": 10000,
            "fields": ["id", "status", "tags", "description"]
        })
    );
    response["output"].clone()
}

fn open_todo(description: &str) -> Value {
    json!({
        "id": "ORB-9",
        "status": "in-progress",
        "tags": ["owner:you", "office-scribe-contradictions"],
        "description": description
    })
}

fn listed(tasks: Vec<Value>, total: u64, truncated: bool) -> Value {
    json!({"tasks": tasks, "total": total, "truncated": truncated})
}

fn pair() -> Vec<Value> {
    vec![
        decision(
            "dec-global-1",
            "workspace pricing",
            "per workspace",
            "2026-10-31T12:00:00Z",
        ),
        decision(
            "dec-global-2",
            "orchard harvest",
            "apples",
            "2026-10-30T12:00:00Z",
        ),
    ]
}

#[test]
fn unresolved_conflict_pulls_the_unranked_partner_and_names_both() {
    // The routine line is lexicographic; the provider accepts either order.
    let description = "Which stands?\nConflicting entries: dec-global-2 dec-global-1\n";
    let out = serve(
        &pair(),
        &[("dec-global-1", 1.0)],
        Some(listed(vec![open_todo(description)], 1, false)),
        8,
    );
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "dec-global-1");
    assert_eq!(rows[1]["id"], "dec-global-2");
    assert_eq!(
        context_of(&rows[0]),
        format!(
            "Options not recorded; only the chosen option is known.\nCustomers asked once\n{}\ntask_type: feature\nrisk_tier: reversible",
            conflict_line("dec-global-2")
        )
    );
    assert_eq!(
        context_of(&rows[1]),
        format!(
            "Options not recorded; only the chosen option is known.\nCustomers asked once\n{}\ntask_type: feature\nrisk_tier: reversible",
            conflict_line("dec-global-1")
        )
    );
    assert_eq!(out["omissions"], json!([]));
    assert_eq!(out["truncated"], false);
}

#[test]
fn a_conflict_that_does_not_fit_keeps_the_ranked_entry_and_still_names_the_partner() {
    let out = serve(
        &pair(),
        &[("dec-global-1", 1.0)],
        Some(listed(
            vec![open_todo("Conflicting entries: dec-global-1 dec-global-2")],
            1,
            false,
        )),
        1,
    );
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "dec-global-1");
    assert!(context_of(&rows[0]).contains(&conflict_line("dec-global-2")));
    assert_eq!(out["truncated"], true);
    assert_eq!(out["omissions"], json!([]));
}

#[test]
fn a_reversed_partner_is_not_named_or_pulled_in_as_a_conflict() {
    let mut entries = pair();
    let mut later = decision(
        "dec-global-3",
        "orchard harvest",
        "pears",
        "2026-10-30T18:00:00Z",
    );
    later["reverses"] = json!("dec-global-2");
    entries.push(later);
    let out = serve(
        &entries,
        &[("dec-global-1", 1.0)],
        Some(listed(
            vec![open_todo("Conflicting entries: dec-global-1 dec-global-2")],
            1,
            false,
        )),
        8,
    );
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "dec-global-1");
    assert_eq!(rows[0]["reversed"], false);
    assert!(!context_of(&rows[0]).contains("Unresolved conflict"));
    assert_eq!(out["truncated"], false);
}

#[test]
fn a_failed_contradiction_read_is_conflicts_unread_and_drops_no_precedent() {
    let out = serve(&pair(), &[("dec-global-1", 1.0)], None, 8);
    assert_eq!(out["precedents"].as_array().unwrap().len(), 1);
    assert_eq!(out["precedents"][0]["id"], "dec-global-1");
    assert!(!context_of(&out["precedents"][0]).contains("Unresolved conflict"));
    assert_eq!(out["omissions"], json!(["conflicts_unread"]));
}

#[test]
fn an_incomplete_list_applies_the_rows_and_reports_conflicts_unread() {
    let out = serve(
        &pair(),
        &[("dec-global-1", 1.0)],
        Some(listed(
            vec![open_todo("Conflicting entries: dec-global-1 dec-global-2")],
            2,
            true,
        )),
        8,
    );
    assert_eq!(out["precedents"].as_array().unwrap().len(), 2);
    assert!(context_of(&out["precedents"][0]).contains(&conflict_line("dec-global-2")));
    assert_eq!(out["omissions"], json!(["conflicts_unread"]));
}

#[test]
fn closed_malformed_and_ineligible_contradiction_todos_are_ignored() {
    let mut agent = decision(
        "dec-global-3",
        "orchard harvest",
        "pears",
        "2026-10-29T12:00:00Z",
    );
    agent["decided_by"] = json!("agent");
    let mut twin = decision(
        "dec-global-4",
        "workspace pricing",
        "per workspace",
        "2026-10-28T12:00:00Z",
    );
    twin["same_as"] = json!("dec-global-1");
    let mut entries = pair();
    entries.push(agent);
    entries.push(twin);
    let tasks = vec![
        json!({
            "id": "ORB-done",
            "status": "done",
            "tags": ["owner:you", "office-scribe-contradictions"],
            "description": "Conflicting entries: dec-global-1 dec-global-2"
        }),
        json!({
            "id": "ORB-tag",
            "status": "proposed",
            "tags": ["owner:you"],
            "description": "Conflicting entries: dec-global-1 dec-global-2"
        }),
        open_todo("The two choices disagree, with no machine-readable line."),
        open_todo("Conflicting entries: dec-global-1 dec-global-2 extra"),
        open_todo("Conflicting entries: dec-global-1 dec-global-1"),
        open_todo("Conflicting entries: dec-global-1 dec-global-3"),
        open_todo("Conflicting entries: dec-global-1 dec-global-4"),
    ];
    let out = serve(
        &entries,
        &[("dec-global-1", 1.0)],
        Some(listed(tasks, 7, false)),
        8,
    );
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "dec-global-1");
    assert!(!context_of(&rows[0]).contains("Unresolved conflict"));
    assert_eq!(out["omissions"], json!([]));
    assert_eq!(out["truncated"], false);
}
