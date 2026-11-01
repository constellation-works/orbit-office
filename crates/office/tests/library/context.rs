use super::library_support::{Env, git};
use std::path::{Path, PathBuf};

/// Tools that open the store. Dispatch reaches the grant check before the handler.
fn store_openers() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("office.reindex", serde_json::json!({})),
        ("office.search", serde_json::json!({"query": "quixotic"})),
        (
            "office.config-set",
            serde_json::json!({"key": "retention.meeting", "value": 30}),
        ),
        (
            "office.knowledge-show",
            serde_json::json!({"id": "dom-acme"}),
        ),
        (
            "office.source-show",
            serde_json::json!({"id": "src-20261028-acme-qbr"}),
        ),
        (
            "office.capture",
            serde_json::json!({
                "kind": "note",
                "title": "Escaped fixture",
                "text": "must not land outside the grant\n",
                "sensitivity": "normal"
            }),
        ),
        ("office.status", serde_json::json!({})),
    ]
}

fn assert_escaped_untouched(escaped: &Path, seeded: bool, state: &Path, label: &str) {
    if seeded {
        assert!(
            escaped.join(".office/config.yaml").is_file(),
            "{label}: seeded config removed"
        );
        assert!(escaped.join(".git").exists(), "{label}: seeded git removed");
        for dir in ["sources", "knowledge", "artifacts", "proposals"] {
            assert!(
                !escaped.join(dir).exists(),
                "{label}: created layout {dir} under {}",
                escaped.display()
            );
        }
    } else {
        assert!(!escaped.exists(), "{label}: created {}", escaped.display());
    }
    assert!(
        !state.join("index.sqlite").exists(),
        "{label}: created the index while refusing"
    );
}

/// A tree `looks_initialized` accepts (config and git) with no layout directories,
/// so an unguarded open would create those directories outside the grant.
fn seed_looking_initialized(root: &Path) {
    std::fs::create_dir_all(root.join(".office")).expect("seed office dir");
    std::fs::write(root.join(".office/config.yaml"), "packs: []\n").expect("seed config");
    git(root, &["init"]);
}

#[test]
fn context_library_dir_outside_the_grant_is_refused_before_any_create() {
    let openers = store_openers();

    // Absolute path outside the workspace, and relative paths that resolve
    // outside {{workspace}}/library (a traversal, and a sibling of library/).
    let missing = [
        ("absolute", true),
        ("relative-traversal", false),
        ("relative-sibling", false),
    ];
    for (label, absolute) in missing {
        let env = Env::new();
        let (library_dir, escaped) = if absolute {
            let escaped = env._dir.path().join("escaped-abs");
            (escaped.to_str().expect("utf8").to_string(), escaped)
        } else if label == "relative-traversal" {
            (
                "../escaped-rel".to_string(),
                env._dir.path().join("escaped-rel"),
            )
        } else {
            ("not-library".to_string(), env.workspace.join("not-library"))
        };
        assert!(!escaped.exists(), "{label} fixture already exists");
        for (tool, input) in &openers {
            let response = env.tool_with_config_library_dir(tool, input.clone(), &library_dir);
            assert_eq!(response["ok"], false, "{label} {tool}: {response}");
            assert_eq!(
                response["error"]["code"],
                serde_json::json!("library_dir_outside_workspace"),
                "{label} {tool}: {response}"
            );
            assert_escaped_untouched(&escaped, false, &env.state, &format!("{label} {tool}"));
        }
    }

    // context.library_dir is the fallback when config omits the key.
    let env = Env::new();
    let escaped = env._dir.path().join("escaped-fallback");
    let response = env.tool_with_context_library_dir(
        "office.reindex",
        serde_json::json!({}),
        escaped.to_str().expect("utf8"),
    );
    assert_eq!(
        response["error"]["code"],
        serde_json::json!("library_dir_outside_workspace"),
        "{response}"
    );
    assert_escaped_untouched(&escaped, false, &env.state, "fallback");

    // An existing tree that already looks initialized must not gain layout.
    let env = Env::new();
    let escaped = env._dir.path().join("escaped-seeded");
    seed_looking_initialized(&escaped);
    let library_dir = escaped.to_str().expect("utf8").to_string();
    for (tool, input) in &openers {
        let response = env.tool_with_config_library_dir(tool, input.clone(), &library_dir);
        assert_eq!(response["ok"], false, "seeded {tool}: {response}");
        assert_eq!(
            response["error"]["code"],
            serde_json::json!("library_dir_outside_workspace"),
            "seeded {tool}: {response}"
        );
        assert_escaped_untouched(&escaped, true, &env.state, &format!("seeded {tool}"));
    }

    // office.init's own input check still refuses, and a context library_dir
    // of `library` still scaffolds the granted root.
    let env = Env::new();
    let refused = env.tool(
        "office.init",
        serde_json::json!({ "library_dir": "../outside" }),
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["error"]["code"],
        serde_json::json!("library_dir_outside_workspace"),
        "{refused}"
    );
    assert!(!env._dir.path().join("outside").exists());

    let env = Env::new();
    let created = env.tool_with_config_library_dir("office.init", serde_json::json!({}), "library");
    assert_eq!(created["ok"], true, "{created}");
    assert_eq!(
        created["output"]["already_initialized"],
        serde_json::json!(false),
        "{created}"
    );
    let library = env.library();
    assert!(library.join(".office/config.yaml").is_file(), "{created}");
    assert!(library.join("sources").is_dir());
    assert!(library.join(".git").is_dir());
    assert!(env.state.join("index.sqlite").is_file());
}

/// Every path and file byte under `root`, for proving a tree unchanged.
fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read snapshot dir") {
            let path = entry.expect("snapshot entry").path();
            let relative = path.strip_prefix(root).expect("under root").to_path_buf();
            if path.is_dir() {
                entries.push((relative, None));
                pending.push(path);
            } else {
                entries.push((relative, Some(std::fs::read(&path).expect("read file"))));
            }
        }
    }
    entries.sort();
    entries
}

#[cfg(unix)]
#[test]
fn symlinked_grant_root_refuses_a_nested_library_dir_before_any_create() {
    // {{workspace}}/library is itself a link to a directory outside the
    // workspace; `library/subdir` names a path below the link, which
    // resolves outside the physical grant.
    let openers = store_openers();
    for seeded in [false, true] {
        let env = Env::new();
        let outside = env._dir.path().join("outside");
        std::fs::create_dir_all(&outside).expect("outside");
        if seeded {
            seed_looking_initialized(&outside.join("subdir"));
        }
        std::os::unix::fs::symlink(&outside, env.library()).expect("symlink grant root");
        let before = snapshot_tree(&outside);

        let mut calls: Vec<(String, serde_json::Value)> = Vec::new();
        for (tool, input) in &openers {
            calls.push((
                format!("config {tool}"),
                env.tool_with_config_library_dir(tool, input.clone(), "library/subdir"),
            ));
        }
        calls.push((
            "context office.reindex".to_string(),
            env.tool_with_context_library_dir(
                "office.reindex",
                serde_json::json!({}),
                "library/subdir",
            ),
        ));
        calls.push((
            "office.init input".to_string(),
            env.tool(
                "office.init",
                serde_json::json!({ "library_dir": "library/subdir" }),
            ),
        ));
        for (label, response) in calls {
            assert_eq!(response["ok"], false, "seeded={seeded} {label}: {response}");
            assert_eq!(
                response["error"]["code"],
                serde_json::json!("library_dir_outside_workspace"),
                "seeded={seeded} {label}: {response}"
            );
            assert_eq!(
                snapshot_tree(&outside),
                before,
                "seeded={seeded} {label}: changed the outside target"
            );
            assert!(
                !env.state.join("index.sqlite").exists(),
                "seeded={seeded} {label}: created the index while refusing"
            );
        }
    }
}
