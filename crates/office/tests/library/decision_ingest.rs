//! Through the built binary: `office.decision-ingest` appends answered
//! Orbit decision requests to the ledgers (specs/decision-precedents.md,
//! Ingesting Orbit decisions and Feedback). A fake `orbit` on PATH serves
//! the callbacks from files under the temp `HOME`; the Library is a temp
//! root.

use super::library_support::{Env, git};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const SOURCE: &str = "src-20261028-ingest-call";

/// A fake `orbit`: `orbit.decision.list` answers `$HOME/orbit/list.json`,
/// or `list-<workspace>.json` for a followed workspace (exit 1 when that
/// file is absent); `orbit.decision.show` answers `show-<id>.json`;
/// `orbit.task.show` answers the asked task as a `feature`. Every call's
/// name and input is logged to `$HOME/orbit/calls.log`.
fn fake_orbit(env: &Env) -> String {
    let bin = env.workspace.join("fake-bin-decisions");
    std::fs::create_dir_all(&bin).expect("bin");
    let script = bin.join("orbit");
    std::fs::write(
        &script,
        r#"#!/bin/sh
name="$3"; input="$7"
dir="$HOME/orbit"
printf '%s %s\n' "$name" "$input" >> "$dir/calls.log"
workspace=$(printf '%s' "$input" | sed -n 's/.*"workspace":"\([^"]*\)".*/\1/p')
id=$(printf '%s' "$input" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
case "$name" in
  orbit.decision.list)
    if [ -n "$workspace" ]; then file="$dir/list-$workspace.json"; else file="$dir/list.json"; fi
    [ -f "$file" ] || exit 1
    cat "$file" ;;
  orbit.decision.show)
    [ -f "$dir/show-$id.json" ] || exit 1
    cat "$dir/show-$id.json" ;;
  orbit.task.show)
    printf '{"id":"%s","type":"feature"}' "$id" ;;
  *) exit 1 ;;
esac
"#,
    )
    .expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"))
}

fn orbit_dir(env: &Env) -> PathBuf {
    let dir = env.workspace.join("home/orbit");
    std::fs::create_dir_all(&dir).expect("orbit dir");
    dir
}

fn serve(env: &Env, file: &str, value: &Value) {
    std::fs::write(orbit_dir(env).join(file), value.to_string()).expect("fixture");
}

fn answers(labels: &[(&str, &str)]) -> Value {
    Value::Array(
        labels
            .iter()
            .map(|(id, label)| json!({"id": id, "label": label, "consequence": format!("{label}: the work proceeds"), "risk_tier": "reversible"}))
            .collect(),
    )
}

/// An answered request.
fn answered(
    id: &str,
    task: &str,
    gate: &str,
    options: Value,
    answer: &str,
    by: &str,
    at: &str,
) -> Value {
    json!({
        "id": id, "task_id": task, "gate": gate, "answers": options,
        "risk_tier": "reversible", "category": format!("ws_acme/{gate}"),
        "summary": format!("Decide {task}."),
        "binding": {"kind": "task_revision", "revision": "r", "status": "blocked"},
        "raised_by": "block_triage", "raised_at": "2026-10-28T08:00:00Z",
        "expires_at": "2026-10-31T08:00:00Z", "state": "answered",
        "resolution": {"request_id": id, "answer_id": answer, "answered_by": by,
                       "provenance": "dashboard", "answered_at": at}
    })
}

fn review(id: &str, task: &str, reviews: &str, answer: &str, at: &str) -> Value {
    let mut request = answered(
        id,
        task,
        "model_answer_review",
        answers(&[("confirm", "Confirm"), ("veto", "Veto")]),
        answer,
        "daniel",
        at,
    );
    request["binding"] = json!({"kind": "request", "request_id": reviews});
    request["summary"] = Value::Null;
    request
}

fn closed(id: &str, state: &str) -> Value {
    json!({"id": id, "task_id": "ORB-9", "gate": "block_triage", "answers": answers(&[("1", "Requeue")]),
           "binding": {"kind": "block_episode", "episode": "e"}, "raised_by": "block_triage",
           "raised_at": "2026-10-28T08:00:00Z", "expires_at": "2026-10-31T08:00:00Z", "state": state})
}

/// An initialized Library following `ws_acme`, with one evidence source
/// and three captured scribe entries: one that matches the human answer on
/// ORB-1 and two that both match the answer on ORB-4.
fn library_with_scribe_entries(env: &Env) {
    let init = env.tool("office.init", json!({}));
    assert_eq!(init["ok"], true, "{init}");
    let set = env.tool(
        "office.config-set",
        json!({"key": "brief.task_workspaces", "value": ["ws_acme"]}),
    );
    assert_eq!(set["ok"], true, "{set}");
    let path = env
        .library()
        .join(format!("sources/doc/2026/10/{SOURCE}/source.md"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "---\nid: {SOURCE}\nkind: doc\ntitle: Ingest call\nsensitivity: normal\n---\nWe go with per workspace pricing.\nShip it on the March train.\nYes, the March train it is.\n"
        ),
    )
    .unwrap();
    assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
    for (line, chosen, task, at) in [
        (
            1,
            "per workspace pricing",
            "ORB-1",
            "2026-10-28T10:00:00+01:00",
        ),
        (2, "the March train", "ORB-4", "2026-10-28T09:05:00Z"),
        (3, "the March train", "ORB-4", "2026-10-28T09:10:00Z"),
    ] {
        let quote = match line {
            1 => "We go with per workspace pricing.",
            2 => "Ship it on the March train.",
            _ => "Yes, the March train it is.",
        };
        let captured = env.tool(
            "office.capture",
            json!({"decision": {
                "question": "What do we do?", "question_by": "scribe", "options": "unrecoverable",
                "chosen": chosen, "quote": quote, "ref": format!("{SOURCE}#L{line}"),
                "scope": "project:acme",
                "context": {"task_type": null, "workspace": "ws_acme", "task": task, "risk_tier": null},
                "decided_by": "human", "decided_at": at
            }}),
        );
        assert_eq!(captured["ok"], true, "{captured}");
    }
}

fn entries(library: &Path, ledger: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(library.join(format!("sources/decisions/{ledger}.md")))
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| line.split_once(" #decision "))
        .map(|(_, json)| serde_json::from_str(json).expect("entry json"))
        .collect()
}

fn by_request<'a>(entries: &'a [Value], request: &str) -> Vec<&'a Value> {
    entries
        .iter()
        .filter(|entry| entry["orbit"]["request_id"] == request)
        .collect()
}

#[test]
fn answered_requests_become_mapped_ledger_entries_with_feedback_links_and_dedupe() {
    let env = Env::new();
    library_with_scribe_entries(&env);
    let library = env.library();
    let path = fake_orbit(&env);
    let tool = |input: Value| env.tool_with_path("office.decision-ingest", input, &path);

    let pricing = answers(&[("1", "per seat"), ("2", "per workspace pricing")]);
    let mut human = answered(
        "dr-human",
        "ORB-1",
        "agent_request",
        pricing,
        "2",
        "daniel",
        "2026-10-28T09:10:00Z",
    );
    human["resolution"]["note"] = json!("seats churn too much");
    human["topic"] = json!("pricing");
    let mut model = answered(
        "dr-model",
        "ORB-2",
        "block_triage",
        answers(&[("1", "Requeue"), ("2", "Archive")]),
        "1",
        "model",
        "2026-10-28T11:00:00Z",
    );
    model["resolution"]["model"] = json!({
        "assessment_id": "as-1", "policy_digest": "sha256:abc", "model": "decision-model@1",
        "probability": 0.91, "cited_precedents": ["dec-acme-1"]
    });
    let vetoed = answered(
        "dr-vetoed",
        "ORB-3",
        "block_triage",
        answers(&[("1", "Requeue"), ("2", "Archive")]),
        "2",
        "model",
        "2026-10-28T11:30:00Z",
    );
    // Listed without its resolution: ingestion reads it with decision.show.
    let mut unresolved = answered(
        "dr-shown",
        "ORB-5",
        "proposed_approval",
        answers(&[("approve", "Approve"), ("archive", "Archive")]),
        "approve",
        "daniel",
        "2026-10-28T12:30:00Z",
    );
    serve(&env, "show-dr-shown.json", &unresolved);
    unresolved["resolution"] = Value::Null;
    let precedent = answered(
        "dr-precedent",
        "ORB-6",
        "block_triage",
        answers(&[("1", "Requeue")]),
        "1",
        "precedent",
        "2026-10-28T10:00:00Z",
    );
    serve(
        &env,
        "list.json",
        &json!({"requests": [
            // Newest first, as Orbit lists them: ingestion orders by answer time.
            review("dr-veto", "ORB-3", "dr-vetoed", "veto", "2026-10-28T12:10:00Z"),
            review("dr-confirm", "ORB-2", "dr-model", "confirm", "2026-10-28T12:00:00Z"),
            human, model, vetoed, unresolved, precedent,
            closed("dr-superseded", "superseded"), closed("dr-expired", "expired"),
            closed("dr-open", "open"),
        ]}),
    );
    serve(
        &env,
        "list-ws_acme.json",
        &json!({"requests": [answered(
            "dr-train", "ORB-4", "agent_request",
            answers(&[("1", "the March train"), ("2", "the April train")]),
            "1", "daniel", "2026-10-28T09:20:00Z",
        )]}),
    );

    let dry = tool(json!({"dry_run": true}));
    assert_eq!(dry["ok"], true, "{dry}");
    assert_eq!(dry["output"]["appended"].as_array().unwrap().len(), 7);
    assert_eq!(
        entries(&library, "acme").len(),
        3,
        "a dry run writes nothing"
    );

    let head = git(&library, &["rev-list", "--count", "HEAD"]);
    let run = tool(json!({}));
    assert_eq!(run["ok"], true, "{run}");
    let output = &run["output"];
    assert_eq!(output["dry_run"], false);
    assert_eq!(output["unavailable"], json!([]));
    let skipped: Vec<(String, String)> = output["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["request_id"].as_str().unwrap().into(),
                s["reason"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        skipped,
        vec![
            ("dr-precedent".into(), "precedent_actor".into()),
            ("dr-superseded".into(), "superseded".into()),
            ("dr-expired".into(), "expired".into()),
        ],
        "{output}"
    );
    assert_eq!(
        git(&library, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse::<u32>()
            .unwrap(),
        head.trim().parse::<u32>().unwrap() + 1,
        "one commit per run"
    );
    assert_eq!(
        git(
            &library,
            &["status", "--porcelain", "--", "sources/decisions"]
        ),
        ""
    );

    let ledger = entries(&library, "acme");
    assert_eq!(ledger.len(), 10, "three scribe entries and seven ingested");
    // The mapping table, on the human answer.
    let human = by_request(&ledger, "dr-human")[0];
    assert_eq!(human["question"], "Decide ORB-1.");
    assert_eq!(human["question_ref"], "orbit-decision:dr-human");
    assert_eq!(human["ref"], "orbit-decision:dr-human");
    assert_eq!(human["source"], "orbit-decision:dr-human");
    assert_eq!(human["scope"], "project:acme");
    assert_eq!(human["chosen"], "per workspace pricing");
    assert_eq!(
        human["quote"],
        "per workspace pricing\nseats churn too much"
    );
    assert_eq!(
        human["options"][1],
        json!({"text": "per workspace pricing", "ref": "orbit-decision:dr-human#2", "id": "2",
               "consequence": "per workspace pricing: the work proceeds"})
    );
    assert_eq!(
        human["context"],
        json!({"task_type": "feature", "workspace": "ws_acme", "task": "ORB-1", "risk_tier": "reversible"})
    );
    assert_eq!(human["decided_by"], "human");
    assert_eq!(human["decided_at"], "2026-10-28T09:10:00Z");
    assert_eq!(
        human["orbit"],
        json!({"request_id": "dr-human", "gate": "agent_request", "category": "ws_acme/agent_request",
               "topic": "pricing", "answered_by": "daniel", "channel": "dashboard"})
    );
    // Dedupe: one scribe match links; two matches link nothing.
    assert_eq!(human["same_as"], "dec-acme-1");
    let train = by_request(&ledger, "dr-train")[0];
    assert!(train.get("same_as").is_none(), "{train}");
    let dedupe = output["dedupe"].as_array().unwrap();
    assert!(dedupe.contains(&json!({"request_id": "dr-human", "entry": human["id"], "outcome": "same_as", "same_as": "dec-acme-1"})), "{output}");
    assert!(dedupe.contains(&json!({"request_id": "dr-train", "entry": train["id"], "outcome": "dedupe_ambiguous", "candidates": ["dec-acme-2", "dec-acme-3"]})), "{output}");
    // Feedback: the model answer, its confirm, and a veto.
    let model = by_request(&ledger, "dr-model")[0];
    assert_eq!(model["decided_by"], "model");
    assert_eq!(model["orbit"]["model"]["probability"], json!(0.91));
    assert_eq!(model["orbit"]["model"]["model"], "decision-model@1");
    let confirm = by_request(&ledger, "dr-confirm")[0];
    assert_eq!(confirm["decided_by"], "human");
    assert_eq!(confirm["confirms"], model["id"]);
    assert_eq!(confirm["reverses"], Value::Null);
    assert_eq!(confirm["chosen"], "Confirm");
    let vetoed = by_request(&ledger, "dr-vetoed")[0];
    let veto = by_request(&ledger, "dr-veto")[0];
    assert_eq!(veto["decided_by"], "human");
    assert_eq!(veto["reverses"], vetoed["id"]);
    assert!(veto.get("confirms").is_none());
    // The resolution decision.show supplied.
    assert_eq!(by_request(&ledger, "dr-shown")[0]["chosen"], "Approve");
    let shown = env.tool_with_path(
        "office.source-show",
        json!({"id": vetoed["id"].clone()}),
        &path,
    );
    assert_eq!(shown["output"]["decisions"][0]["reversed_by"], veto["id"]);

    // A human re-answers the vetoed request; everything else is already in.
    let list: Value =
        serde_json::from_str(&std::fs::read_to_string(orbit_dir(&env).join("list.json")).unwrap())
            .unwrap();
    let mut requests = list["requests"].as_array().unwrap().clone();
    for request in &mut requests {
        if request["id"] == "dr-vetoed" {
            request["resolution"] = json!({"request_id": "dr-vetoed", "answer_id": "1",
                "answered_by": "daniel", "provenance": "mail", "answered_at": "2026-10-28T13:00:00Z"});
        }
    }
    serve(&env, "list.json", &json!({"requests": requests}));
    let again = tool(json!({}));
    assert_eq!(again["ok"], true, "{again}");
    let appended = again["output"]["appended"].as_array().unwrap();
    assert_eq!(appended.len(), 1, "{again}");
    let ledger = entries(&library, "acme");
    let answers = by_request(&ledger, "dr-vetoed");
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[1]["decided_by"], "human");
    assert_eq!(answers[1]["chosen"], "Requeue");
    assert_eq!(answers[1]["reverses"], vetoed["id"]);
    let already = again["output"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["reason"] == "already_ingested")
        .count();
    assert_eq!(already, 6, "every other answer is already in: {again}");

    // `since` drops answers applied before it.
    let late = tool(json!({"since": "2026-10-29T00:00:00Z", "dry_run": true}));
    assert_eq!(late["output"]["appended"], json!([]));
    assert_eq!(late["output"]["skipped"], json!([]));
}

#[test]
fn an_unreadable_followed_workspace_is_reported_and_an_unreadable_office_workspace_refuses() {
    let env = Env::new();
    library_with_scribe_entries(&env);
    let path = fake_orbit(&env);
    // No list-ws_acme.json: the followed workspace cannot be read.
    serve(&env, "list.json", &json!({"requests": []}));
    let run = env.tool_with_path("office.decision-ingest", json!({}), &path);
    assert_eq!(run["ok"], true, "{run}");
    assert_eq!(run["output"]["unavailable"], json!(["ws_acme"]));
    assert_eq!(run["output"]["appended"], json!([]));

    std::fs::remove_file(orbit_dir(&env).join("list.json")).unwrap();
    let refused = env.tool_with_path("office.decision-ingest", json!({}), &path);
    assert_eq!(refused["ok"], false, "{refused}");
}
