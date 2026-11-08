use super::super::library_support::Env;

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
