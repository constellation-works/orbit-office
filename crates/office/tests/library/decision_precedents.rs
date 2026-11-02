//! Provider contract exercised through the built binary with isolated ledgers.
use super::library_support::{Env, git};
use serde_json::{Value, json};

const AS_OF: &str = "2026-11-01T12:00:00Z";

fn request(k: usize) -> Value {
    json!({"contract":"orbit.precedent-provider/1", "workspace_id":"ws_acme",
        "request":{"id":"request-example", "gate":"agent_request", "category":"ws_acme/pricing",
            "question":"workspace pricing", "options":[{"id":"ship","label":"ship","consequence":"Release"}],
            "topic":null, "raised_at":AS_OF}, "as_of":AS_OF, "k":k})
}

fn entry(n: usize) -> Value {
    json!({"id":format!("dec-global-{n}"), "question":"workspace pricing", "question_by":"scribe",
        "options":"unrecoverable", "chosen":"per workspace", "quote":"per workspace",
        "ref":"SCR-example#m-1", "scope":"global", "context":{"task_type":"feature","workspace":"ws_acme","task":null,"risk_tier":"reversible"},
        "decided_by":"human", "decided_at":"2026-10-31T12:00:00Z", "reverses":null, "source":"SCR-example"})
}

fn seed(env: &Env, slug: &str, entries: &[Value]) -> String {
    let scope = if slug == "global" {
        "global".into()
    } else {
        format!("project:{slug}")
    };
    let mut text = format!(
        "---\nid: decisions-{slug}\nkind: decisions\nscope: {scope}\ncreated_at: 2026-10-01T12:00:00Z\n---\n"
    );
    for entry in entries {
        text.push_str(&format!(
            "- {} {} #decision {entry}\n",
            entry["id"].as_str().unwrap(),
            entry["decided_at"].as_str().unwrap()
        ));
    }
    let dir = env.library().join("sources/decisions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{slug}.md")), &text).unwrap();
    text
}

/// A fake `orbit` prepended to PATH. A missing fixture is an empty task list.
/// `$HOME/orbit/tasks.json` replaces that list. `$HOME/orbit/fail` exits 1.
/// Other tools stay on the inherited PATH.
fn orbit_path(env: &Env) -> String {
    let bin = env.workspace.join("fake-bin-precedents");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("orbit");
    std::fs::write(
        &script,
        r#"#!/bin/sh
dir="$HOME/orbit"
mkdir -p "$dir"
printf '%s %s\n' "$3" "$7" >> "$dir/calls.log"
if [ -f "$dir/fail" ]; then
  exit 1
fi
if [ -f "$dir/tasks.json" ]; then
  cat "$dir/tasks.json"
  exit 0
fi
printf '%s\n' '{"tasks":[],"total":0,"truncated":false}'
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

fn precedents(env: &Env, input: Value) -> Value {
    env.tool_with_path("office.decision-precedents", input, &orbit_path(env))
}

fn output(env: &Env, k: usize) -> Value {
    let response = precedents(env, request(k));
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

#[test]
fn provider_contract_is_closed_and_rejects_bad_bounds() {
    let env = Env::with_initialized_library();
    let mut unsupported = request(8);
    unsupported["contract"] = json!("orbit.precedent-provider/2");
    assert_eq!(
        precedents(&env, unsupported)["error"]["code"],
        "precedent_contract_unsupported"
    );
    let mut inputs = vec![request(0), request(9)];
    for pointer in ["/extra", "/request/extra", "/request/options/0/extra"] {
        let mut input = request(8);
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        input
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(true));
        inputs.push(input);
    }
    for (pointer, value) in [
        ("/as_of", json!("2026-11-02T12:00:00Z")),
        ("/request/options", json!([])),
        ("/request/question", json!("x".repeat(8193))),
        ("/request/options/0/id", json!("x".repeat(65))),
        ("/workspace_id", json!("\n")),
        ("/k", json!(1.5)),
    ] {
        let mut input = request(8);
        *input.pointer_mut(pointer).unwrap() = value;
        inputs.push(input);
    }
    for input in inputs {
        let refused = precedents(&env, input.clone());
        assert_eq!(
            refused["error"]["code"], "invalid_argument",
            "{input}: {refused}"
        );
    }
}

#[test]
fn candidates_exclude_private_automation_future_other_projects_and_ingested_groups() {
    let env = Env::with_initialized_library();
    let mut entries: Vec<Value> = (1..=12).map(entry).collect();
    entries[1]["decided_by"] = json!("agent");
    entries[2]["decided_by"] = json!("model");
    entries[3]["private"] = json!(true);
    for key in ["question", "question_by", "options", "chosen", "quote"] {
        entries[3].as_object_mut().unwrap().remove(key);
    }
    entries[4]["decided_at"] = json!(AS_OF);
    entries[5]["decided_at"] = json!("2026-11-02T12:00:00Z");
    entries[6]["source"] = json!("orbit-decision:example");
    entries[6]["same_as"] = json!("dec-global-8");
    entries[8]["same_as"] = json!("dec-global-8");
    entries[9]["confirms"] = json!("dec-global-3");
    // Confirmed model entry is eligible, but the confirmation itself is feedback.
    entries[10]["orbit"] = json!({"gate":"model_answer_review"});
    entries[11]["question"] = json!("bananas");
    entries[11]["chosen"] = json!("oranges");
    entries[11]["quote"] = json!("oranges");
    seed(&env, "global", &entries);
    let mut other = entry(1);
    other["id"] = json!("dec-other-1");
    other["scope"] = json!("project:other");
    seed(&env, "other", &[other]);
    let mut project = entry(1);
    project["id"] = json!("dec-acme-1");
    project["scope"] = json!("project:acme");
    seed(&env, "acme", &[project]);
    let out = output(&env, 8);
    let ids: Vec<&str> = out["precedents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["dec-acme-1", "dec-global-1", "dec-global-3"]);
    assert_eq!(out["precedents"][2]["decided_by_kind"], "human_confirmed");
    assert_eq!(out["omissions"], json!([]));
    assert_eq!(out["truncated"], false);
    // A confirmation after as_of cannot legitimize a historical model answer.
    entries[9]["decided_at"] = json!("2026-11-02T12:00:00Z");
    seed(&env, "global", &entries);
    assert_eq!(output(&env, 8)["precedents"].as_array().unwrap().len(), 2);
}

#[test]
fn recency_and_validated_half_life_change_the_top_k() {
    let env = Env::with_initialized_library();
    let mut old = entry(1);
    old["decided_at"] = json!("2026-08-03T12:00:00Z");
    let mut recent = entry(2);
    recent["context"]["workspace"] = Value::Null;
    seed(&env, "global", &[old, recent]);
    assert_eq!(output(&env, 1)["precedents"][0]["id"], "dec-global-2");
    let settings_before = std::fs::read(env.library().join(".office/config.yaml")).unwrap();
    let head_before = git(&env.library(), &["rev-parse", "HEAD"]);
    for value in [
        json!(0),
        json!(3651),
        json!(-1),
        json!(1.5),
        json!("90"),
        json!(true),
    ] {
        let out = env.tool(
            "office.config-set",
            json!({"key":"scribe.precedents.half_life_days", "value":value}),
        );
        assert_eq!(out["error"]["code"], "invalid_library_setting", "{out}");
        assert_eq!(
            std::fs::read(env.library().join(".office/config.yaml")).unwrap(),
            settings_before
        );
        assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head_before);
    }
    for value in [json!(3650), json!(1), Value::Null] {
        let out = env.tool(
            "office.config-set",
            json!({"key":"scribe.precedents.half_life_days", "value":value}),
        );
        assert_eq!(out["ok"], true, "{out}");
        let expected = if value == 3650 {
            "dec-global-1"
        } else {
            "dec-global-2"
        };
        assert_eq!(output(&env, 1)["precedents"][0]["id"], expected);
    }
}

#[test]
fn a_reversal_flags_the_whole_same_as_group_and_private_ingestion_excludes_its_twins() {
    let env = Env::with_initialized_library();
    let mut duplicate = entry(2);
    duplicate["same_as"] = json!("dec-global-1");
    let mut reversing = entry(3);
    reversing["reverses"] = json!("dec-global-2");
    reversing["question"] = json!("new direction");
    reversing["chosen"] = json!("change plans");
    reversing["quote"] = json!("change plans");
    seed(&env, "global", &[entry(1), duplicate, reversing]);
    let out = output(&env, 8);
    assert_eq!(out["precedents"].as_array().unwrap().len(), 2);
    assert_eq!(out["precedents"][0]["id"], "dec-global-1");
    assert_eq!(out["precedents"][0]["reversed"], true);
    assert_eq!(out["precedents"][1]["id"], "dec-global-3");

    let mut twin = entry(1);
    twin["same_as"] = json!("dec-acme-1");
    seed(&env, "global", &[twin]);
    let mut ingested = entry(1);
    ingested["id"] = json!("dec-acme-1");
    ingested["scope"] = json!("project:acme");
    ingested["source"] = json!("orbit-decision:example");
    let text = seed(&env, "acme", &[ingested]).replace(
        "kind: decisions\n",
        "kind: decisions\nsensitivity: private\n",
    );
    std::fs::write(env.library().join("sources/decisions/acme.md"), text).unwrap();
    assert_eq!(
        output(&env, 8)["precedents"],
        json!([]),
        "private-ledger ingestion is still a duplicate exclusion"
    );
}

#[test]
fn half_life_is_checked_when_reading_hand_edited_library_settings() {
    for precedents in [
        json!({"half_life_days":0}),
        json!({"half_life_days":3651}),
        json!({"half_life_days":"90"}),
        json!({"extra":90}),
    ] {
        let env = Env::with_initialized_library();
        let path = env.library().join(".office/config.yaml");
        std::fs::write(
            path,
            json!({"scribe":{"precedents":precedents}}).to_string(),
        )
        .unwrap();
        let out = env.tool("office.status", json!({}));
        assert_eq!(out["error"]["code"], "invalid_library_setting", "{out}");
    }
}

#[test]
fn reversals_pull_in_a_non_matching_entry_and_never_use_future_or_private_text() {
    let env = Env::with_initialized_library();
    let mut reversing = entry(3);
    reversing["question"] = json!("different direction");
    reversing["chosen"] = json!("new plan");
    reversing["quote"] = json!("new plan");
    reversing["reverses"] = json!("dec-global-1");
    seed(&env, "global", &[entry(1), entry(2), reversing.clone()]);
    let out = output(&env, 2);
    assert_eq!(out["precedents"][0]["id"], "dec-global-1");
    assert_eq!(out["precedents"][0]["reversed"], true);
    assert_eq!(out["precedents"][1]["id"], "dec-global-3");
    let one = output(&env, 1);
    assert_eq!(one["precedents"][0]["id"], "dec-global-3");
    assert_eq!(one["truncated"], true);
    reversing["decided_at"] = json!("2026-11-02T12:00:00Z");
    seed(&env, "global", &[entry(1), entry(2), reversing]);
    assert_eq!(output(&env, 2)["precedents"][0]["reversed"], false);

    let mut private = entry(3);
    private["private"] = json!(true);
    private["reverses"] = json!("dec-global-1");
    for key in ["question", "question_by", "options", "chosen", "quote"] {
        private.as_object_mut().unwrap().remove(key);
    }
    seed(&env, "global", &[entry(1), entry(2), private]);
    let out = output(&env, 2);
    assert_eq!(out["precedents"][0]["reversed"], true);
    assert_eq!(out["precedents"][1]["id"], "dec-global-2");
    assert_eq!(out["omissions"], json!(["decisions-global"]));
}

#[test]
fn options_citations_and_response_shape_preserve_evidence_without_recommendations() {
    let env = Env::with_initialized_library();
    let mut exact = entry(2);
    exact["chosen"] = json!("  PER   WORKSPACE ");
    exact["quote"] = exact["chosen"].clone();
    exact["options"] = json!([{"text":"per seat", "ref":"SCR-example#m-1"},{"text":"per workspace", "ref":"SCR-example#m-1"}]);
    let mut unmatched = entry(3);
    unmatched["options"] = exact["options"].clone();
    unmatched["chosen"] = json!("flat fee");
    unmatched["quote"] = json!("flat fee");
    let text = seed(&env, "global", &[entry(1), exact, unmatched]);
    let before = git(&env.library(), &["status", "--porcelain"]);
    let out = output(&env, 8);
    let mut keys: Vec<_> = out
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["contract", "omissions", "precedents", "truncated"]);
    let rows = out["precedents"].as_array().unwrap();
    for row in rows {
        assert_eq!(row.as_object().unwrap().len(), 11, "{row}");
        assert_eq!(row["scope"]["workspace_id"], "ws_acme");
        assert_eq!(row["scope"]["category"], "ws_acme/pricing");
        assert_eq!(row["decided_by"], "user");
        let id = row["id"].as_str().unwrap();
        let raw = text
            .lines()
            .find(|line| line.starts_with(&format!("- {id} ")))
            .unwrap();
        assert_eq!(
            row["citation"]["digest"],
            format!(
                "sha256:{}",
                office_core::domain::source::sha256_hex(raw.as_bytes())
            )
        );
        assert_eq!(row["citation"]["source"], "office-decision-ledger");
        assert_eq!(
            row["citation"]["ref"],
            format!("{id} decisions-global#L{}", id.rsplit('-').next().unwrap())
        );
    }
    let find = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(
        find("dec-global-1")["options"],
        json!([{"id":"chosen","label":"per workspace"}])
    );
    assert!(
        find("dec-global-1")["context"]
            .as_str()
            .unwrap()
            .starts_with("Options not recorded; only the chosen option is known.")
    );
    assert_eq!(find("dec-global-2")["chosen_option"], "o2");
    assert_eq!(find("dec-global-3")["chosen_option"], "chosen");
    assert!(
        find("dec-global-3")["context"]
            .as_str()
            .unwrap()
            .contains("per seat")
    );
    assert_eq!(
        git(&env.library(), &["status", "--porcelain"]),
        before,
        "read-only scan writes no Library state"
    );
    assert_eq!(output(&env, 8), out, "stable evidence");
}

#[test]
fn invalid_ledgers_are_omitted_and_utf8_bounds_and_response_size_are_enforced() {
    let env = Env::with_initialized_library();
    let mut huge = entry(1);
    huge["question"] = json!(format!("workspace {}", "é".repeat(5000)));
    huge["because"] = json!({"text":"理".repeat(5000),"ref":"SCR-example#m-1"});
    seed(&env, "global", &[huge]);
    std::fs::write(env.library().join("sources/decisions/broken.md"), "invalid").unwrap();
    std::fs::write(
        env.library().join("sources/decisions/unreadable.md"),
        [0xff, 0xfe],
    )
    .unwrap();
    let out = output(&env, 8);
    assert_eq!(
        out["omissions"],
        json!(["decisions-broken", "decisions-unreadable"])
    );
    assert_eq!(out["truncated"], true);
    assert!(out["precedents"][0]["question"].as_str().unwrap().len() <= 8192);
    assert!(out["precedents"][0]["context"].as_str().unwrap().len() <= 8192);
}

#[test]
fn fts_overlap_category_group_dedupe_and_tie_breaking_rank_evidence() {
    let env = Env::with_initialized_library();
    let mut preferred = entry(2);
    preferred["orbit"] = json!({"category":"ws_acme/pricing","topic":"pricing"});
    let mut duplicate = entry(3);
    duplicate["same_as"] = json!("dec-global-2");
    duplicate["orbit"] = preferred["orbit"].clone();
    let mut unrelated = entry(4);
    unrelated["question"] = json!("orchards");
    unrelated["chosen"] = json!("apples");
    unrelated["quote"] = json!("apples");
    seed(&env, "global", &[entry(1), preferred, duplicate, unrelated]);
    let out = output(&env, 8);
    assert_eq!(out["precedents"].as_array().unwrap().len(), 2);
    assert_eq!(
        out["precedents"][0]["id"], "dec-global-2",
        "category bonus; duplicate group counts once"
    );
    assert_eq!(out["precedents"][0]["scope"]["topic"], "pricing");
    assert_eq!(out["precedents"][1]["id"], "dec-global-1");
    let mut query = request(8);
    query["request"]["question"] = json!("zebra");
    query["request"]["options"][0]["label"] = json!("gazelle");
    assert_eq!(
        precedents(&env, query)["output"]["precedents"],
        json!([]),
        "no lexical overlap drops every candidate"
    );
}

#[test]
fn containment_and_ambiguous_options_do_not_invent_a_labelled_choice() {
    let env = Env::with_initialized_library();
    let mut contained = entry(1);
    contained["options"] = json!([{"text":"Use per workspace pricing", "ref":"SCR-example#m-1"},{"text":"Use per seat pricing", "ref":"SCR-example#m-1"}]);
    let mut ambiguous = entry(2);
    ambiguous["options"] = json!([{"text":"per workspace pricing", "ref":"SCR-example#m-1"},{"text":"per workspace licenses", "ref":"SCR-example#m-1"}]);
    seed(&env, "global", &[contained, ambiguous]);
    let out = output(&env, 8);
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(
        rows.iter().find(|r| r["id"] == "dec-global-1").unwrap()["chosen_option"],
        "o1"
    );
    let ambiguous = rows.iter().find(|r| r["id"] == "dec-global-2").unwrap();
    assert_eq!(
        ambiguous["options"],
        json!([{"id":"chosen","label":"per workspace"}])
    );
    assert!(
        ambiguous["context"]
            .as_str()
            .unwrap()
            .contains("Recovered options:")
    );
}

#[test]
fn response_and_omission_caps_mark_gaps_without_exceeding_the_host_contract() {
    let env = Env::with_initialized_library();
    let entries: Vec<Value> = (1..=8).map(|n| {
        let mut d = entry(n);
        let options: Vec<Value> = (1..=8).map(|i|json!({"text":format!("option {i} {}", "é".repeat(4090)),"ref":"SCR-example#m-1"})).collect();
        d["chosen"] = options[0]["text"].clone(); d["quote"] = d["chosen"].clone();
        d["options"] = json!(options);
        d
    }).collect();
    seed(&env, "global", &entries);
    for n in 1..=33 {
        std::fs::write(
            env.library()
                .join(format!("sources/decisions/broken-{n}.md")),
            "invalid",
        )
        .unwrap();
    }
    let out = output(&env, 8);
    assert!(serde_json::to_vec(&out).unwrap().len() <= 262_144);
    assert!(out["precedents"].as_array().unwrap().len() < 8);
    assert!(!out["precedents"].as_array().unwrap().is_empty());
    assert_eq!(out["omissions"].as_array().unwrap().len(), 32);
    assert_eq!(out["truncated"], true);
}

#[cfg(unix)]
#[test]
fn provider_reports_symlink_omissions_and_reads_without_state_or_writer_locks() {
    use std::os::unix::fs::symlink;
    let env = Env::with_initialized_library();
    seed(&env, "global", &[entry(1)]);
    let outside = env.workspace.join("private.md");
    std::fs::write(&outside, "Private text must not be returned").unwrap();
    symlink(&outside, env.library().join("sources/decisions/linked.md")).unwrap();
    let fifo = std::process::Command::new("mkfifo")
        .arg(env.library().join("sources/decisions/pipe.md"))
        .env_clear()
        .env("HOME", env.workspace.join("home"))
        .env("PATH", std::env::var("PATH").unwrap())
        .output()
        .unwrap();
    assert!(fifo.status.success(), "mkfifo fixture: {fifo:?}");
    let absent = env.workspace.join("absent-state");
    let response = env.run_envelope(
        "office.decision-precedents",
        request(8),
        json!({"workspace_root":env.workspace,"plugin_state":absent}),
        &orbit_path(&env),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        response["output"]["omissions"],
        json!(["decisions-linked", "decisions-pipe"])
    );
    assert_eq!(
        response["output"]["precedents"].as_array().unwrap().len(),
        1
    );
    assert!(
        !absent.exists(),
        "provider creates no disk index, writer state or lock"
    );
    assert_eq!(
        std::fs::read_to_string(outside).unwrap(),
        "Private text must not be returned"
    );
    let out = env.tool(
        "office.config-set",
        json!({"key":"sensitivity.decisions","value":"private"}),
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(output(&env, 8)["precedents"], json!([]));
}

fn contradiction_todo() -> Value {
    json!({
        "tasks": [{
            "id": "ORB-9",
            "status": "in-progress",
            "tags": ["owner:you", "office-scribe-contradictions"],
            "description": "Which stands?\nConflicting entries: dec-global-1 dec-global-2\n"
        }],
        "total": 1,
        "truncated": false
    })
}

#[test]
fn an_open_contradiction_pulls_the_partner_through_the_orbit_callback() {
    let env = Env::with_initialized_library();
    let mut partner = entry(2);
    partner["question"] = json!("orchard harvest");
    partner["chosen"] = json!("apples");
    partner["quote"] = json!("apples");
    seed(&env, "global", &[entry(1), partner]);
    let dir = env.workspace.join("home/orbit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tasks.json"), contradiction_todo().to_string()).unwrap();
    let out = output(&env, 8);
    let rows = out["precedents"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["dec-global-1", "dec-global-2"]
    );
    assert_eq!(
        rows[0]["context"],
        "Options not recorded; only the chosen option is known.\nUnresolved conflict with dec-global-2; the user has not said which stands.\ntask_type: feature\nrisk_tier: reversible"
    );
    assert_eq!(
        rows[1]["context"],
        "Options not recorded; only the chosen option is known.\nUnresolved conflict with dec-global-1; the user has not said which stands.\ntask_type: feature\nrisk_tier: reversible"
    );
    assert_eq!(out["omissions"], json!([]));
    assert_eq!(out["truncated"], false);
    let log = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert!(log.contains("orbit.task.list"), "{log}");
}

#[test]
fn a_failed_orbit_contradiction_read_is_conflicts_unread() {
    let env = Env::with_initialized_library();
    let mut partner = entry(2);
    partner["question"] = json!("orchard harvest");
    partner["chosen"] = json!("apples");
    partner["quote"] = json!("apples");
    seed(&env, "global", &[entry(1), partner]);
    let dir = env.workspace.join("home/orbit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tasks.json"), contradiction_todo().to_string()).unwrap();
    std::fs::write(dir.join("fail"), "1").unwrap();
    let out = output(&env, 8);
    assert_eq!(
        out["precedents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["dec-global-1"]
    );
    assert!(
        !out["precedents"][0]["context"]
            .as_str()
            .unwrap()
            .contains("Unresolved conflict")
    );
    assert_eq!(out["omissions"], json!(["conflicts_unread"]));
}
