use super::super::library_support::Env;
use super::{RENEWAL_PAGE, git, library_with_source};

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
