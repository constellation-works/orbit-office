use super::library_support::{Env, RENEWAL_PAGE, git};
use std::path::PathBuf;

/// An initialized Library holding one meeting source, indexed.
fn library_with_source(env: &Env) -> PathBuf {
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let library = env.library();
    let source = library.join("sources/2026/10/src-20261028-acme-qbr/source.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(
        &source,
        "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\nrelated: [dom-project-acme-renewal]\n---\n\n[00:10:00] Jane: Renewal is in March.\n\n[00:12:31] Jane: Price matters most.\n",
    )
    .unwrap();
    let reindex = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(reindex["output"]["indexed_documents"], serde_json::json!(1));
    library
}

#[test]
fn knowledge_write_refuses_unresolved_cites_without_writing_and_commits_a_valid_page_once() {
    let env = Env::new();
    let library = library_with_source(&env);
    let head = git(&library, &["rev-parse", "HEAD"]);
    let page = library.join("knowledge/domain/dom-project-acme-renewal.md");

    for (missing, cite) in [
        ("source", "src-20261028-missing#t=00:12:31"),
        ("anchor", "src-20261028-acme-qbr#t=01:00:00"),
        ("anchor", "src-20261028-acme-qbr#L99"),
    ] {
        let markdown = RENEWAL_PAGE.replace(
            "src-20261028-acme-qbr#t=00:12:31\nrefs",
            &format!("{cite}\nrefs"),
        );
        let refused = env.tool(
            "office.knowledge-write",
            serde_json::json!({ "markdown": markdown }),
        );
        assert_eq!(
            refused["ok"],
            serde_json::json!(false),
            "{missing}: {refused}"
        );
        assert_eq!(
            refused["error"]["code"],
            serde_json::json!("unresolved_cite"),
            "{missing}: {refused}"
        );
        assert!(!page.exists(), "a refused write leaves no page ({missing})");
        assert_eq!(
            git(&library, &["rev-parse", "HEAD"]),
            head,
            "no commit ({missing})"
        );
    }

    let written = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": RENEWAL_PAGE }),
    );
    assert_eq!(written["ok"], serde_json::json!(true), "{written}");
    assert_eq!(
        written["output"],
        serde_json::json!({
            "id": "dom-project-acme-renewal",
            "type": "domain",
            "path": "knowledge/domain/dom-project-acme-renewal.md",
            "created": true,
        })
    );
    assert_eq!(std::fs::read_to_string(&page).unwrap(), RENEWAL_PAGE);
    let commits = git(
        &library,
        &["rev-list", "--count", &format!("{}..HEAD", head.trim())],
    );
    assert_eq!(commits.trim(), "1", "a valid write commits once");
    assert_eq!(
        git(&library, &["show", "--name-only", "--format=", "HEAD"]).trim(),
        "knowledge/domain/dom-project-acme-renewal.md"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["domain"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["library"]["pages_off_template"],
        serde_json::json!(0)
    );
}

#[test]
fn knowledge_show_returns_the_model_view_and_the_sources_and_pages_linking_to_it() {
    let env = Env::new();
    let _library = library_with_source(&env);
    let written = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": RENEWAL_PAGE }),
    );
    assert_eq!(written["ok"], serde_json::json!(true), "{written}");
    let prep = "---\nid: rb-client-qbr-prep\ntitle: QBR prep\ncites: [dom-project-acme-renewal]\nrelated: [dom-project-acme-renewal]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: draft\n---\n## Inputs\nThe renewal page.\n";
    let runbook = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": prep }),
    );
    assert_eq!(runbook["ok"], serde_json::json!(true), "{runbook}");
    assert_eq!(
        runbook["output"]["path"],
        serde_json::json!("knowledge/runbooks/rb-client-qbr-prep.md")
    );

    let shown = env.tool(
        "office.knowledge-show",
        serde_json::json!({ "id": "dom-project-acme-renewal" }),
    );
    assert_eq!(shown["ok"], serde_json::json!(true), "{shown}");
    let output = &shown["output"];
    assert_eq!(
        output["backlinks"],
        serde_json::json!([
            {
                "id": "rb-client-qbr-prep",
                "kind": "runbook",
                "title": "QBR prep",
                "path": "knowledge/runbooks/rb-client-qbr-prep.md",
                "via": ["cites", "related"],
            },
            {
                "id": "src-20261028-acme-qbr",
                "kind": "source",
                "title": "Acme QBR",
                "path": "sources/2026/10/src-20261028-acme-qbr/source.md",
                "via": ["related"],
            },
        ])
    );
    assert_eq!(output["page"]["template"], serde_json::json!("project"));
    assert_eq!(
        output["page"]["refs"],
        serde_json::json!([
            {"label": "Renewal tickets", "target": "account", "value": "jira:ACME", "authoritative": false, "as_of": null},
            {"label": "Jane Park", "target": "contact", "value": null, "authoritative": false, "as_of": null},
        ])
    );
    assert!(
        !shown.to_string().contains("jane@acme.example"),
        "the contact value never leaves through show: {shown}"
    );

    let missing = env.tool(
        "office.knowledge-show",
        serde_json::json!({ "id": "dom-absent" }),
    );
    assert_eq!(
        missing["error"]["code"],
        serde_json::json!("page_not_found")
    );
}

/// A fake `orbit` for proposal filing: no dismissals, sequential proposal
/// ids, and each artifact put copied under `$HOME/puts/<id>.json`.
fn fake_orbit_proposals(env: &Env) -> String {
    let bin = env.workspace.join("fake-bin-proposals");
    std::fs::create_dir_all(&bin).expect("bin");
    let script = bin.join("orbit");
    std::fs::write(
        &script,
        r#"#!/bin/sh
name="$3"; input="$7"
mkdir -p "$HOME/calls" "$HOME/puts"
printf '%s\n' "$name" >> "$HOME/calls/log"
case "$name" in
  orbit.task.list)
    printf '%s' '{"tasks":[],"truncated":false}' ;;
  orbit.task.add)
    n=0; [ -f "$HOME/calls/count" ] && n=$(cat "$HOME/calls/count"); n=$((n+1)); printf '%s' "$n" > "$HOME/calls/count"
    id="ORB-PROP$n"
    printf '%s' "$input" > "$HOME/calls/add-$id.json"
    printf '{"id":"%s","status":"proposed"}' "$id" ;;
  orbit.task.artifact.put)
    id=$(printf '%s' "$input" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
    src=$(printf '%s' "$input" | sed -n 's/.*"source_path":"\([^"]*\)".*/\1/p')
    cp "$src" "$HOME/puts/$id.json" || exit 1
    printf '%s' '{"ok":true}' ;;
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

#[test]
fn knowledge_verify_digest_pins_proposals_and_stamps_through_the_binary() {
    use office_core::domain::source::sha256_hex;
    let env = Env::with_initialized_library();
    let library = env.library();
    let drifted = "---\nid: dom-verify-drifted\ntitle: Drifted fixture\n---\nThe weekly report is produced by `make old-report`.\n";
    let current = "---\nid: dom-verify-current\ntitle: Current fixture\n---\nThe weekly report is produced by `make report`.\n";
    let uncheckable =
        "---\nid: dom-verify-uncheckable\ntitle: Uncheckable fixture\n---\nTeam morale is high.\n";
    let private = "---\nid: dom-verify-private\ntitle: Private fixture\nsensitivity: private\n---\nSecret claim.\n";
    for markdown in [drifted, current, uncheckable, private] {
        let written = env.tool(
            "office.knowledge-write",
            serde_json::json!({ "markdown": markdown }),
        );
        assert_eq!(written["ok"], serde_json::json!(true), "{written}");
    }
    let path = fake_orbit_proposals(&env);
    let tool = |tool: &str, input: serde_json::Value| env.tool_with_path(tool, input, &path);
    // The digest knowledge-show returns is the hash of the file's exact
    // bytes, for the private page too, whose body stays withheld.
    let mut digests = std::collections::BTreeMap::new();
    for (id, markdown) in [
        ("dom-verify-drifted", drifted),
        ("dom-verify-current", current),
        ("dom-verify-uncheckable", uncheckable),
        ("dom-verify-private", private),
    ] {
        let shown = tool("office.knowledge-show", serde_json::json!({ "id": id }));
        assert_eq!(shown["ok"], serde_json::json!(true), "{shown}");
        assert_eq!(
            shown["output"]["page"]["sha256"],
            serde_json::json!(sha256_hex(markdown.as_bytes())),
            "{shown}"
        );
        digests.insert(
            id,
            shown["output"]["page"]["sha256"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let private_shown = tool(
        "office.knowledge-show",
        serde_json::json!({ "id": "dom-verify-private" }),
    );
    assert_eq!(
        private_shown["output"]["content_withheld"],
        serde_json::json!(true)
    );
    assert_eq!(
        private_shown["output"]["page"]["body"],
        serde_json::json!("")
    );

    // Drifted: a knowledge_update pinned to the shown digest, quoting the
    // contradicting evidence, is accepted for creation.
    let evidence =
        "observable: `make old-report` has no such target; the report target is `make report`";
    let created = tool(
        "office.proposal-create",
        serde_json::json!({
            "title": "Correct the drifted report command",
            "description": format!("The page claims `make old-report`; {evidence}."),
            "evidence": ["dom-verify-drifted", evidence],
            "payload": {"kind": "knowledge_update", "markdown": drifted.replace("make old-report", "make report"), "base_sha256": digests["dom-verify-drifted"]},
        }),
    );
    assert_eq!(created["ok"], serde_json::json!(true), "{created}");
    assert_eq!(
        created["output"]["created"],
        serde_json::json!(true),
        "{created}"
    );
    let home = env.workspace.join("home");
    let filed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.join("puts/ORB-PROP1.json")).expect("drift proposal"),
    )
    .expect("proposal json");
    assert_eq!(
        filed["base_sha256"],
        serde_json::json!(digests["dom-verify-drifted"])
    );
    let add_input =
        std::fs::read_to_string(home.join("calls/add-ORB-PROP1.json")).expect("add input");
    assert!(add_input.contains("make old-report"), "{add_input}");

    // A stale digest is refused before anything is filed.
    let stale = tool(
        "office.proposal-create",
        serde_json::json!({
            "title": "Stale correction",
            "description": "Pinned to a digest that is not the page's.",
            "evidence": ["dom-verify-drifted"],
            "payload": {"kind": "knowledge_update", "markdown": drifted.replace("make old-report", "make report"), "base_sha256": "0".repeat(64)},
        }),
    );
    assert_eq!(stale["ok"], serde_json::json!(false), "{stale}");
    assert_eq!(
        stale["error"]["code"],
        serde_json::json!("proposal_revision_changed"),
        "{stale}"
    );

    // Current: verification is recorded as a stamp-only knowledge_update
    // proposal (same page, last_verified set), never a direct write.
    let stamp = current.replace(
        "title: Current fixture\n",
        "title: Current fixture\nlast_verified: 2026-11-01\n",
    );
    let stamped = tool(
        "office.proposal-create",
        serde_json::json!({
            "title": "Record verification of Current fixture",
            "description": "Claims checked against observable evidence; stamp only.",
            "evidence": ["dom-verify-current", "observable: `make report` is the report target"],
            "payload": {"kind": "knowledge_update", "markdown": stamp, "base_sha256": digests["dom-verify-current"]},
        }),
    );
    assert_eq!(
        stamped["output"]["created"],
        serde_json::json!(true),
        "{stamped}"
    );

    // Uncheckable and private pages get no proposal: report entries only.
    let report = serde_json::json!([
        {"id": "dom-verify-drifted", "verdict": "drifted", "evidence": evidence, "proposal": created["output"]["id"]},
        {"id": "dom-verify-current", "verdict": "current", "evidence": "observable: `make report` is the report target", "proposal": stamped["output"]["id"]},
        {"id": "dom-verify-uncheckable", "verdict": "unverifiable", "reason": "no checkable claim: morale is not observable by this run"},
        {"id": "dom-verify-private", "verdict": "unverifiable", "reason": "content_withheld: private page body is withheld"},
    ]);
    assert_eq!(report.as_array().unwrap().len(), 4);
    assert_eq!(
        report
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["verdict"] == "unverifiable")
            .count(),
        2
    );
    // No page file was written directly: every page keeps its fixture bytes,
    // and exactly two proposals were filed.
    for (id, markdown) in [
        ("dom-verify-drifted", drifted),
        ("dom-verify-current", current),
        ("dom-verify-uncheckable", uncheckable),
        ("dom-verify-private", private),
    ] {
        let page_path = library.join(format!("knowledge/domain/{id}.md"));
        assert_eq!(
            std::fs::read(&page_path).unwrap(),
            markdown.as_bytes(),
            "{id}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(home.join("calls/count")).unwrap(),
        "2",
        "only the drifted and current pages produced proposals"
    );
}

/// A fake `orbit` on PATH for the callback adapter. It knows one task,
/// refuses any other as `task_not_found`, and files each artifact put under
/// `$HOME/puts/<path>`, copying the staged file while it still exists.
fn fake_orbit(env: &Env) -> String {
    let bin = env.workspace.join("fake-bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let script = bin.join("orbit");
    std::fs::write(
        &script,
        r#"#!/bin/sh
name="$3"; input="$7"
field() { printf '%s' "$input" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }
mkdir -p "$HOME/calls"
printf '%s\n' "$name" >> "$HOME/calls/log"
case "$name" in
  orbit.task.show)
    if [ "$(field id)" = "T-1" ]; then
      printf '%s' '{"id":"T-1","title":"Prepare the Acme renewal","description":"Draft the renewal note.","tags":["project:acme-renewal","source:src-20261028-acme-qbr"]}'
    else
      printf '%s' '{"error":"task not found","code":"task_not_found"}' >&2
      exit 1
    fi ;;
  orbit.task.artifact.put)
    path="$(field path)"
    mkdir -p "$HOME/puts/$(dirname "$path")"
    cp "$(field source_path)" "$HOME/puts/$path" || exit 1
    printf '%s' '{"ok":true}' ;;
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

#[test]
fn search_and_attach_through_the_binary_copy_model_views_at_the_library_commit() {
    let env = Env::new();
    let library = library_with_source(&env);
    let page = RENEWAL_PAGE.replace(
        "aliases: [the renewal]\n",
        "aliases: [the renewal]\ntags: [project:acme-renewal]\n",
    );
    let written = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": page }),
    );
    assert_eq!(written["ok"], serde_json::json!(true), "{written}");

    let found = env.tool(
        "office.search",
        serde_json::json!({ "query": "renewal", "template": "project" }),
    );
    assert_eq!(found["ok"], serde_json::json!(true), "{found}");
    assert_eq!(
        found["output"]["hits"][0]["id"],
        serde_json::json!("dom-project-acme-renewal")
    );
    assert_eq!(found["output"]["count"], serde_json::json!(1));
    let sources = env.tool(
        "office.search",
        serde_json::json!({ "query": "renewal", "kind": "source", "until": "2026-10-28" }),
    );
    assert_eq!(
        sources["output"]["count"],
        serde_json::json!(1),
        "{sources}"
    );
    let empty = env.tool("office.search", serde_json::json!({ "query": " " }));
    assert_eq!(
        empty["error"]["code"],
        serde_json::json!("invalid_argument")
    );

    let path = fake_orbit(&env);
    let home = env.workspace.join("home");
    let missing = env.tool_with_path(
        "office.attach",
        serde_json::json!({ "task": "T-404" }),
        &path,
    );
    assert_eq!(
        missing["error"]["code"],
        serde_json::json!("task_not_found"),
        "{missing}"
    );
    assert!(
        !home.join("puts").exists(),
        "nothing copied for an unknown task"
    );

    let attached = env.tool_with_path("office.attach", serde_json::json!({ "task": "T-1" }), &path);
    assert_eq!(attached["ok"], serde_json::json!(true), "{attached}");
    let head = git(&library, &["rev-parse", "HEAD"]);
    assert_eq!(
        attached["output"]["library_commit"],
        serde_json::json!(head.trim())
    );
    let reasons: Vec<(&str, &str)> = attached["output"]["attached"]
        .as_array()
        .expect("attached")
        .iter()
        .map(|file| {
            (
                file["id"].as_str().unwrap(),
                file["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        reasons,
        [
            ("dom-project-acme-renewal", "tagged"),
            ("src-20261028-acme-qbr", "source"),
        ]
    );
    let copy = std::fs::read_to_string(
        home.join("puts/context/knowledge/domain/dom-project-acme-renewal.md"),
    )
    .expect("page copy");
    assert!(!copy.contains("jane@acme.example"), "{copy}");
    assert!(
        copy.contains("{ label: \"Jane Park\", contact: null }"),
        "{copy}"
    );
    let source = std::fs::read_to_string(
        home.join("puts/context/sources/2026/10/src-20261028-acme-qbr/source.md"),
    )
    .expect("source copy");
    assert_eq!(
        source,
        std::fs::read_to_string(library.join("sources/2026/10/src-20261028-acme-qbr/source.md"))
            .expect("source")
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.join("puts/office-attach.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    assert_eq!(manifest["library_commit"], serde_json::json!(head.trim()));
    assert_eq!(manifest["task"], serde_json::json!("T-1"));
    let staging = library.join(".office/attach");
    assert_eq!(std::fs::read_dir(&staging).expect("staging").count(), 0);
    let status = git(
        &library,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert!(!status.contains(".office/attach"), "{status}");
    assert_eq!(
        git(&library, &["rev-parse", "HEAD"]),
        head,
        "attach commits nothing"
    );
}
