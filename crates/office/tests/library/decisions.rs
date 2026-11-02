//! Through the built binary: decision ledgers (specs/scribe.md, Decision
//! ledger). `office.capture`'s `decision` input appends one D13 entry whose
//! verbatim fields are checked against the cited anchor, reads derive
//! `reversed_by`, a kindless paste is refused, and `office.init` moves
//! retired `note` sources into the ledgers once.

use super::library_support::{Env, git};
use serde_json::{Value, json};
use std::path::Path;

const SOURCE: &str = "src-20261028-pricing-call";

/// An initialized Library holding one meeting-notes source, indexed.
fn library_with_evidence(env: &Env) {
    let init = env.tool("office.init", json!({}));
    assert_eq!(init["ok"], true, "{init}");
    let path = env
        .library()
        .join(format!("sources/doc/2026/10/{SOURCE}/source.md"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "---\nid: {SOURCE}\nkind: doc\ntitle: Pricing call\nsensitivity: normal\n---\nShould we price per seat or per workspace?\nOptions were per seat, or per workspace.\nWe go with per workspace pricing from March.\nSeat counts churn too much to bill on.\n"
        ),
    )
    .unwrap();
    let reindex = env.tool("office.reindex", json!({}));
    assert_eq!(reindex["ok"], true, "{reindex}");
}

fn decision(quote: &str) -> Value {
    json!({
        "question": "Should we price per seat or per workspace?",
        "question_ref": format!("{SOURCE}#L1"),
        "options": [
            {"text": "per seat", "ref": format!("{SOURCE}#L2")},
            {"text": "per workspace", "ref": format!("{SOURCE}#L2")}
        ],
        "chosen": "per workspace pricing",
        "quote": quote,
        "ref": format!("{SOURCE}#L3"),
        "because": {"text": "Seat counts churn too much to bill on.", "ref": format!("{SOURCE}#L4")},
        "scope": "project:acme",
        "context": {"task_type": "feature", "workspace": "acme", "task": "<task>", "risk_tier": "low"},
        "decided_by": "human",
        "decided_at": "2026-10-28T10:00:00+01:00"
    })
}

fn capture(env: &Env, decision: Value) -> Value {
    env.tool("office.capture", json!({ "decision": decision }))
}

#[test]
fn a_decision_entry_round_trips_every_d13_field_and_reads_derive_reversed_by() {
    let env = Env::new();
    library_with_evidence(&env);
    let library = env.library();
    let quote = "We go with per workspace pricing from March.";

    let first = capture(&env, decision(quote));
    assert_eq!(first["ok"], true, "{first}");
    let output = &first["output"];
    assert_eq!(output["id"], "dec-acme-1");
    assert_eq!(output["kind"], "decisions");
    assert_eq!(output["ledger"], "decisions-acme");
    assert_eq!(output["path"], "sources/decisions/acme.md");

    let mut expected = decision(quote);
    expected["id"] = json!("dec-acme-1");
    expected["source"] = json!(SOURCE);
    expected["reverses"] = Value::Null;
    let shown = env.tool("office.source-show", json!({"id": "dec-acme-1"}));
    assert_eq!(shown["ok"], true, "{shown}");
    let entry = &shown["output"]["decisions"][0];
    for (field, value) in expected.as_object().unwrap() {
        assert_eq!(&entry[field], value, "{field} round-trips: {entry}");
    }
    assert_eq!(entry["reversed_by"], Value::Null);

    let mut reversal = decision(quote);
    reversal["reverses"] = json!("dec-acme-1");
    let second = capture(&env, reversal);
    assert_eq!(second["ok"], true, "{second}");
    assert_eq!(second["output"]["id"], "dec-acme-2");

    let ledger = std::fs::read_to_string(library.join("sources/decisions/acme.md")).unwrap();
    assert!(
        !ledger.contains("reversed_by"),
        "reversed_by is derived, never written:\n{ledger}"
    );
    let entries: Vec<&str> = ledger
        .lines()
        .filter(|line| line.starts_with("- dec-acme-"))
        .collect();
    assert_eq!(entries.len(), 2, "one line per entry:\n{ledger}");
    assert!(entries[0].starts_with("- dec-acme-1 2026-10-28T10:00:00+01:00 #decision {"));

    let whole = env.tool("office.source-show", json!({"id": "decisions-acme"}));
    assert_eq!(whole["ok"], true, "{whole}");
    let views = whole["output"]["decisions"].as_array().unwrap();
    assert_eq!(views.len(), 2);
    assert_eq!(views[0]["reversed_by"], "dec-acme-2");
    assert_eq!(views[1]["reverses"], "dec-acme-1");
    assert_eq!(
        git(
            &library,
            &["status", "--porcelain", "--", "sources/decisions"]
        ),
        "",
        "each append is committed"
    );

    let page = "---\nid: dom-project-acme-pricing\ntype: domain\ntemplate: project\ntitle: Acme pricing\ncites:\n  - dec-acme-1\n---\n## Status\nPer workspace pricing [dec-acme-2].\n\n## Background\nDecided in the call [decisions-acme#L1].\n";
    let written = env.tool("office.knowledge-write", json!({ "markdown": page }));
    assert_eq!(written["ok"], true, "dec- citations resolve: {written}");
}

#[test]
fn a_decision_whose_verbatim_fields_do_not_match_its_evidence_is_refused_without_writing() {
    let env = Env::new();
    library_with_evidence(&env);
    let library = env.library();
    let head = git(&library, &["rev-parse", "HEAD"]);
    let mut wrong_ref = decision("We go with per workspace pricing from March.");
    wrong_ref["ref"] = json!("src-20261028-missing#L1");
    let mut reverses_nothing = decision("We go with per workspace pricing from March.");
    reverses_nothing["reverses"] = json!("dec-acme-9");
    let mut model = decision("We go with per workspace pricing from March.");
    model["decided_by"] = json!("model");
    let mut unknown = decision("We go with per workspace pricing from March.");
    unknown["reversed_by"] = json!("dec-acme-2");
    for (code, input) in [
        (
            "quote_mismatch",
            json!({"decision": decision("We go with per seat pricing from March.")}),
        ),
        ("decision_ref_unresolved", json!({ "decision": wrong_ref })),
        (
            "decision_not_found",
            json!({ "decision": reverses_nothing }),
        ),
        ("decided_by_mismatch", json!({ "decision": model })),
        ("invalid_decision", json!({ "decision": unknown })),
        (
            "invalid_argument",
            json!({"decision": decision("We go with per workspace pricing from March."), "kind": "doc"}),
        ),
        (
            "decision_input_required",
            json!({"kind": "decisions", "title": "A decision", "text": "Ship it."}),
        ),
    ] {
        let refused = env.tool("office.capture", input);
        assert_eq!(refused["ok"], false, "{code}: {refused}");
        assert_eq!(refused["error"]["code"], code, "{refused}");
        assert_eq!(git(&library, &["rev-parse", "HEAD"]), head, "{code}");
    }
    assert!(!library.join("sources/decisions").exists());
}

#[test]
fn a_plain_text_capture_without_a_kind_is_refused_naming_the_paste_kinds_and_the_decision_input() {
    let env = Env::with_initialized_library();
    let library = env.library();
    let head = git(&library, &["rev-parse", "HEAD"]);
    let refused = env.tool(
        "office.capture",
        json!({"title": "Loose text", "text": "We agreed to ship on Friday."}),
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["error"]["code"], "source_kind_required",
        "{refused}"
    );
    let message = refused["error"]["message"].as_str().unwrap();
    for word in ["chat", "email", "doc", "decision"] {
        assert!(message.contains(word), "{word} named: {message}");
    }
    assert_eq!(git(&library, &["rev-parse", "HEAD"]), head);
    let accepted = env.tool(
        "office.capture",
        json!({"kind": "chat", "title": "Loose text", "text": "We agreed to ship on Friday."}),
    );
    assert_eq!(accepted["ok"], true, "{accepted}");
}

fn seed_note(library: &Path, id: &str, extra: &str, body: &str) {
    let path = library.join(format!("sources/note/2026/10/{id}/source.md"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("---\nid: {id}\nkind: note\ntitle: {id} title\n{extra}---\n{body}"),
    )
    .unwrap();
}

#[test]
fn init_moves_note_sources_into_the_ledgers_once_and_old_ids_keep_resolving() {
    let env = Env::with_initialized_library();
    let library = env.library();
    let global = "src-20261027-standup-note";
    let project = "src-20261028-acme-note";
    let private = "src-20261028-private-note";
    seed_note(
        &library,
        global,
        "sensitivity: normal\noccurred_at: 2026-10-27T09:00:00Z\n",
        "Stand-ups move to 10am.\nFridays stay async.\n",
    );
    seed_note(
        &library,
        project,
        "sensitivity: normal\ntags: [project:acme]\n",
        "Acme ships on the March train.\n",
    );
    seed_note(
        &library,
        private,
        "sensitivity: private\n",
        "Private body.\n",
    );
    git(&library, &["add", "--", "sources"]);
    git(
        &library,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "seed notes",
        ],
    );

    let migrated = env.tool("office.init", json!({}));
    assert_eq!(migrated["ok"], true, "{migrated}");
    assert_eq!(
        migrated["output"]["already_initialized"], false,
        "{migrated}"
    );
    assert!(
        !library
            .join(format!("sources/note/2026/10/{global}"))
            .exists()
    );
    assert!(
        !library
            .join(format!("sources/note/2026/10/{project}"))
            .exists()
    );
    assert!(
        library
            .join(format!("sources/note/2026/10/{private}/source.md"))
            .exists(),
        "a private note is left in place"
    );
    assert_eq!(git(&library, &["status", "--porcelain"]), "", "one commit");
    let ledger = std::fs::read_to_string(library.join("sources/decisions/global.md")).unwrap();
    assert!(
        ledger.contains(&format!("  {global}: dec-global-1")),
        "{ledger}"
    );
    let acme = std::fs::read_to_string(library.join("sources/decisions/acme.md")).unwrap();
    assert!(acme.contains(&format!("  {project}: dec-acme-1")), "{acme}");

    for (old, entry, quote) in [
        (
            global,
            "dec-global-1",
            "Stand-ups move to 10am.\nFridays stay async.",
        ),
        (project, "dec-acme-1", "Acme ships on the March train."),
    ] {
        let shown = env.tool("office.source-show", json!({"id": old}));
        assert_eq!(shown["ok"], true, "{shown}");
        assert_eq!(shown["output"]["id"], entry, "{shown}");
        assert_eq!(shown["output"]["moved_from"], old, "{shown}");
        let decision = &shown["output"]["decisions"][0];
        assert_eq!(decision["source"], old);
        assert_eq!(decision["quote"], quote);
        assert_eq!(decision["decided_by"], "human");
        assert_eq!(decision["options"], "unrecoverable");
        let anchored = env.tool("office.source-show", json!({"id": old, "anchor": "L1"}));
        assert_eq!(anchored["ok"], true, "{anchored}");
        assert_eq!(anchored["output"]["id"], entry, "{anchored}");
        assert_eq!(anchored["output"]["span"], quote.lines().next().unwrap());
    }
    let dated = env.tool("office.source-show", json!({"id": "dec-global-1"}));
    assert_eq!(
        dated["output"]["decisions"][0]["decided_at"],
        "2026-10-27T09:00:00Z"
    );

    let page = format!(
        "---\nid: dom-project-acme-train\ntype: domain\ntemplate: project\ntitle: Acme train\ncites:\n  - {global}\n  - {project}#L1\n---\n## Status\nAcme ships on the March train [{project}#L1].\n\n## Background\nStand-ups moved [{global}#L1], Fridays stay async [{global}#L2], see [{project}].\n"
    );
    let written = env.tool("office.knowledge-write", json!({ "markdown": page }));
    assert_eq!(
        written["ok"], true,
        "old ids pass the citation checks: {written}"
    );
    let unresolved = page.replace(&format!("[{global}#L2]"), &format!("[{global}#L9]"));
    let unresolved = unresolved.replace("dom-project-acme-train", "dom-project-acme-other");
    let refused = env.tool("office.knowledge-write", json!({ "markdown": unresolved }));
    assert_eq!(refused["error"]["code"], "unresolved_cite", "{refused}");

    let head = git(&library, &["rev-parse", "HEAD"]);
    let ledgers = (ledger, acme);
    let again = env.tool("office.init", json!({}));
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["output"]["already_initialized"], true, "{again}");
    assert_eq!(
        git(&library, &["rev-parse", "HEAD"]),
        head,
        "a re-run commits nothing"
    );
    assert_eq!(
        (
            std::fs::read_to_string(library.join("sources/decisions/global.md")).unwrap(),
            std::fs::read_to_string(library.join("sources/decisions/acme.md")).unwrap()
        ),
        ledgers,
        "a re-run changes nothing"
    );
}

#[test]
fn preference_pages_resolve_scr_tags_and_ledger_entries_through_knowledge_write() {
    let env = Env::new();
    library_with_evidence(&env);
    let captured = capture(
        &env,
        decision("We go with per workspace pricing from March."),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let scr = env
        .library()
        .join("sources/conversation/2026/11/SCR-20261101-2231-0a3104.md");
    std::fs::create_dir_all(scr.parent().unwrap()).unwrap();
    std::fs::write(&scr, "---\nid: SCR-20261101-2231-0a3104\nkind: conversation\ntitle: Preferences\n---\n## Summary\nConcise.\n## Tags\n### Preferences\n- T1 #preferences concise replies (m-1)\n```text\n- T2 an example, not an entry\n```\n## Next\n- T3 not a tag\n").unwrap();
    assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
    let markdown = "---\nid: dom-preferences-acme\ntitle: Acme preferences\ntemplate: preferences\n---\n## replies\nstatement: Prefer concise replies.\ncites: [\"SCR-20261101-2231-0a3104#T1\", dec-acme-1]\nstated_at: 2026-11-01\nreview_by: 2027-01-30\n";
    let written = env.tool("office.knowledge-write", json!({"markdown":markdown}));
    assert_eq!(written["ok"], true, "{written}");
    assert_eq!(
        written["output"]["path"],
        "knowledge/domain/dom-preferences-acme.md"
    );
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    for anchor in ["T0", "T01", "T2", "T3", "T999"] {
        let refused = env.tool(
            "office.knowledge-write",
            json!({"markdown":markdown.replace("#T1", &format!("#{anchor}"))}),
        );
        assert_eq!(refused["error"]["code"], "unresolved_cite", "{refused}");
        assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    }
    let missing = env.tool(
        "office.knowledge-write",
        json!({"markdown":markdown.replace("review_by: 2027-01-30\n", "")}),
    );
    assert_eq!(missing["error"]["code"], "invalid_argument", "{missing}");
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    // Reindex and read preserve the section-only citations for backlink users.
    assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
    let shown = env.tool(
        "office.knowledge-show",
        json!({"id":"dom-preferences-acme"}),
    );
    assert_eq!(shown["ok"], true, "{shown}");
}
