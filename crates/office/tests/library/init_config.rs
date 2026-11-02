use super::library_support::{BUNDLED_TEMPLATES, DOMAIN, Env, SOURCE, git};
use std::path::{Path, PathBuf};

#[test]
fn config_set_updates_one_setting_and_commits_only_config() {
    let env = Env::with_initialized_library();
    let library = env.library();
    std::fs::write(library.join("unrelated.md"), "keep staged").unwrap();
    git(&library, &["add", "--", "unrelated.md"]);
    for (key, value) in [
        ("retention.meeting", serde_json::json!(30)),
        ("sensitivity.decisions", serde_json::json!("private")),
        ("retention.meeting", serde_json::Value::Null),
    ] {
        let before = git(&library, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse::<u32>()
            .unwrap();
        let response = env.tool(
            "office.config-set",
            serde_json::json!({"key": key, "value": value}),
        );
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(
            response["output"],
            serde_json::json!({"path": ".office/config.yaml", "key": key})
        );
        let text = std::fs::read_to_string(library.join(".office/config.yaml")).unwrap();
        let settings = office_core::domain::settings::Settings::from_yaml(&text).unwrap();
        let encoded = serde_json::to_value(settings).unwrap();
        let (section, name) = key.split_once('.').unwrap();
        assert_eq!(encoded[section][name], value);
        assert_eq!(encoded["thresholds"]["window_days"], 30);
        assert_eq!(
            git(&library, &["rev-list", "--count", "HEAD"])
                .trim()
                .parse::<u32>()
                .unwrap(),
            before + 1
        );
        assert_eq!(
            git(
                &library,
                &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
            )
            .trim(),
            ".office/config.yaml"
        );
        assert_eq!(git(&library, &["show", "HEAD:.office/config.yaml"]), text);
        assert_eq!(
            git(&library, &["diff", "--cached", "--name-only"]).trim(),
            "unrelated.md"
        );
    }
}

#[test]
fn config_set_refusals_leave_config_and_head_unchanged() {
    let env = Env::with_initialized_library();
    let library = env.library();
    let config = std::fs::read(library.join(".office/config.yaml")).unwrap();
    let head = git(&library, &["rev-parse", "HEAD"]);
    for (input, code) in [
        (
            serde_json::json!({"key": "retention.typo", "value": 10}),
            "unknown_library_setting",
        ),
        (
            serde_json::json!({"key": "meta_api_key", "value": "secret-fixture-value"}),
            "unknown_library_setting",
        ),
        (
            serde_json::json!({"key": "open_weights_token", "value": "secret-fixture-value"}),
            "unknown_library_setting",
        ),
        (
            serde_json::json!({"key": "retention.meeting", "value": -1}),
            "invalid_library_setting",
        ),
        (
            serde_json::json!({"key": "sensitivity.decisions", "value": "secret-fixture-value"}),
            "invalid_library_setting",
        ),
        (
            serde_json::json!({"key": "retention.meeting"}),
            "invalid_argument",
        ),
        (
            serde_json::json!({"key": 2, "value": 10}),
            "invalid_argument",
        ),
        (
            serde_json::json!({"key": "retention.meeting", "value": 10, "secret": "secret-fixture-value"}),
            "invalid_argument",
        ),
    ] {
        let response = env.tool("office.config-set", input);
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["code"], code, "{response}");
        assert!(!response.to_string().contains("secret-fixture-value"));
        assert_eq!(
            std::fs::read(library.join(".office/config.yaml")).unwrap(),
            config
        );
        assert_eq!(git(&library, &["rev-parse", "HEAD"]), head);
        assert!(!env.state.join("library.pending-write.json").exists());
        assert!(git(&library, &["status", "--porcelain"]).is_empty());
    }
}

#[test]
fn config_set_waits_for_the_writer_lock_before_changing_settings() {
    let env = Env::with_initialized_library();
    let library = env.library();
    let head = git(&library, &["rev-parse", "HEAD"]);
    let config = std::fs::read(library.join(".office/config.yaml")).unwrap();
    let lock = std::fs::File::create(env.state.join("library.writer.lock")).unwrap();
    lock.lock().unwrap();
    let input = serde_json::json!({"key": "retention.meeting", "value": 45});
    let refused = env.tool("office.config-set", input.clone());
    assert_eq!(
        refused["error"]["code"], "library_lock_timeout",
        "{refused}"
    );
    assert_eq!(
        std::fs::read(library.join(".office/config.yaml")).unwrap(),
        config
    );
    assert_eq!(git(&library, &["rev-parse", "HEAD"]), head);
    assert!(!env.state.join("library.pending-write.json").exists());
    drop(lock);
    let updated = env.tool("office.config-set", input);
    assert_eq!(updated["ok"], true, "{updated}");
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "2");
}

#[test]
fn init_second_run_is_an_already_initialized_no_op() {
    let env = Env::new();
    // A library_dir outside {{workspace}}/library is refused first, typed.
    let refused = env.tool(
        "office.init",
        serde_json::json!({ "library_dir": "../outside" }),
    );
    assert_eq!(refused["ok"], serde_json::json!(false));
    assert_eq!(
        refused["error"]["code"],
        serde_json::json!("library_dir_outside_workspace")
    );
    assert!(
        !env.workspace.join("../outside").exists()
            || !env._dir.path().join("outside").join(".office").exists()
    );

    let first = env.tool("office.init", serde_json::json!({}));
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    assert_eq!(
        first["output"]["already_initialized"],
        serde_json::json!(false)
    );
    let library = env.library();
    for dir in [
        "sources",
        "knowledge/domain",
        "knowledge/runbooks",
        "artifacts",
        "proposals",
        ".office",
    ] {
        assert!(library.join(dir).is_dir(), "layout {dir}");
    }
    for keep in [
        "sources/.gitkeep",
        "knowledge/domain/.gitkeep",
        "knowledge/runbooks/.gitkeep",
        "artifacts/.gitkeep",
        "proposals/.gitkeep",
    ] {
        assert!(library.join(keep).exists(), "gitkeep {keep}");
    }
    assert_eq!(
        std::fs::read_to_string(library.join(".gitignore")).unwrap(),
        "original.*\nframes/\n.office/state/\n"
    );
    let config = std::fs::read_to_string(library.join(".office/config.yaml")).unwrap();
    assert!(config.contains("retention:"), "default config: {config}");
    assert!(library.join(".git").is_dir());
    assert!(env.state.join("index.sqlite").exists());
    assert_eq!(
        git(&library, &["log", "-1", "--format=%s"]).trim(),
        "Initialize the Library"
    );
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");

    // Existing files are never overwritten by a second run.
    std::fs::write(
        library.join(".office/config.yaml"),
        "packs: [meetings, receipts]\n",
    )
    .unwrap();
    std::fs::write(
        library.join(".gitignore"),
        "custom-ignore\nframes/\n.office/state/\n",
    )
    .unwrap();
    std::fs::write(library.join("knowledge/domain/dom-keep.md"), DOMAIN).unwrap();
    let head_before = git(&library, &["rev-parse", "HEAD"]);

    let second = env.tool("office.init", serde_json::json!({}));
    assert_eq!(
        second["output"]["already_initialized"],
        serde_json::json!(true),
        "{second}"
    );
    assert_eq!(second["output"]["created"], serde_json::json!([]));
    assert_eq!(git(&library, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        std::fs::read_to_string(library.join(".office/config.yaml")).unwrap(),
        "packs: [meetings, receipts]\n"
    );
    assert_eq!(
        std::fs::read_to_string(library.join(".gitignore")).unwrap(),
        "custom-ignore\nframes/\n.office/state/\n"
    );
    assert_eq!(
        std::fs::read_to_string(library.join("knowledge/domain/dom-keep.md")).unwrap(),
        DOMAIN
    );
}

#[test]
fn init_lock_timeout_leaves_no_scaffold_and_retry_commits() {
    let env = Env::new();
    std::fs::create_dir_all(&env.state).expect("plugin state");
    let lock =
        std::fs::File::create(env.state.join("library.writer.lock")).expect("writer lock file");
    lock.lock().expect("hold writer lock in another process");

    let refused = env.tool("office.init", serde_json::json!({}));
    assert_eq!(refused["ok"], serde_json::json!(false), "{refused}");
    assert_eq!(
        refused["error"]["code"],
        serde_json::json!("library_lock_timeout"),
        "{refused}"
    );
    for path in [
        ".office/config.yaml",
        ".gitignore",
        "sources",
        "knowledge",
        "artifacts",
        "proposals",
        ".office",
        ".git",
    ] {
        assert!(
            !env.library().join(path).exists(),
            "init wrote {path} while another process held the writer lock"
        );
    }
    assert!(!env.library().exists(), "init created the Library root");
    assert!(!env.state.join("index.sqlite").exists());
    drop(lock);

    let retry = env.tool("office.init", serde_json::json!({}));
    assert_eq!(retry["ok"], serde_json::json!(true), "{retry}");
    assert_eq!(
        retry["output"]["already_initialized"],
        serde_json::json!(false)
    );
    assert_eq!(
        git(&env.library(), &["log", "-1", "--format=%s"]).trim(),
        "Initialize the Library"
    );
    assert_eq!(
        git(&env.library(), &["rev-list", "--count", "HEAD"]).trim(),
        "1"
    );
    assert!(git(&env.library(), &["status", "--porcelain"]).is_empty());
}

#[test]
fn init_recovers_pending_write_while_restoring_missing_template() {
    let env = Env::new();
    let first = env.tool("office.init", serde_json::json!({}));
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    let template = env.library().join(".office/templates/person.md");
    let original_template = std::fs::read(&template).expect("bundled template");
    std::fs::remove_file(&template).expect("remove template for init to restore");
    let relative = "knowledge/domain/recovered.md";
    let journal_path = env.state.join("library.pending-write.json");
    let journal = serde_json::json!({
        "version": 1,
        "root": env.library().canonicalize().expect("Library root"),
        "relative": relative,
        "text": DOMAIN,
        "token": "123-456"
    });
    std::fs::write(&journal_path, journal.to_string()).expect("interrupted write decision");

    let restored = env.tool("office.init", serde_json::json!({}));
    assert_eq!(restored["ok"], serde_json::json!(true), "{restored}");
    assert_eq!(
        restored["output"]["created"],
        serde_json::json!([".office/templates/person.md"])
    );
    assert_eq!(std::fs::read(&template).unwrap(), original_template);
    assert_eq!(
        std::fs::read_to_string(env.library().join(relative)).unwrap(),
        DOMAIN
    );
    assert!(!journal_path.exists(), "init must finish recovery");
    assert_eq!(
        git(
            &env.library(),
            &["show", "HEAD:knowledge/domain/recovered.md"]
        ),
        DOMAIN
    );
    assert_eq!(
        git(&env.library(), &["rev-list", "--count", "HEAD"]).trim(),
        "2"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(true), "{status}");
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(1)
    );
}

#[test]
fn missing_git_is_a_typed_refusal_before_any_creation() {
    let env = Env::new();
    let response = env.tool_with_path(
        "office.init",
        serde_json::json!({}),
        "/nonexistent-office-test",
    );
    assert_eq!(response["ok"], serde_json::json!(false));
    assert_eq!(
        response["error"]["code"],
        serde_json::json!("git_missing"),
        "{response}"
    );
    assert!(!env.library().exists());
}

#[test]
fn reindex_after_index_deletion_makes_documents_counted_again() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let library = env.library();
    let source = library.join("sources/2026/10/src-20261028-acme-qbr/source.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, SOURCE).unwrap();
    std::fs::write(library.join("knowledge/domain/dom-acme.md"), DOMAIN).unwrap();

    let first = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(
        first["output"]["indexed_documents"],
        serde_json::json!(2),
        "{first}"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(
        status["output"]["library"]["initialized"],
        serde_json::json!(true)
    );
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(2)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["source"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["domain"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["secrets"]["meta_api_key"],
        serde_json::json!("unset")
    );

    // Delete the index: status (which opens it) now counts nothing, and a
    // reindex rebuilds it from the files so the documents are indexed again.
    std::fs::remove_file(env.state.join("index.sqlite")).unwrap();
    for suffix in ["index.sqlite-wal", "index.sqlite-shm"] {
        let _ = std::fs::remove_file(env.state.join(suffix));
    }
    let rebuilt = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(
        rebuilt["output"]["indexed_documents"],
        serde_json::json!(2),
        "{rebuilt}"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(2)
    );
}

#[test]
fn reindex_refuses_an_uninitialized_library() {
    let env = Env::new();
    let response = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(response["ok"], serde_json::json!(false));
    assert_eq!(
        response["error"]["code"],
        serde_json::json!("library_not_initialized")
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("office.init"))
    );
}

fn bundled_template(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../office-core/templates")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read bundled {name}: {error}"))
}

fn markdown_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(
                    path.strip_prefix(root)
                        .expect("relative")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    found.sort();
    found
}

#[test]
fn init_lists_the_office_directory_it_creates_without_a_plugin_state() {
    // With no plugin state dir the writer state defaults to `.office/state`
    // inside the Library, so `.office` must still be reported as created.
    let env = Env::new();
    let path = std::env::var("PATH").expect("PATH");
    let context = serde_json::json!({ "workspace_root": env.workspace });
    let init = env.run_envelope("office.init", serde_json::json!({}), context, &path);
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let created = init["output"]["created"].as_array().expect("created");
    assert!(created.iter().any(|item| item == ".office"), "{init}");
}

#[test]
fn init_writes_every_bundled_template_and_no_knowledge_page_in_one_commit() {
    let env = Env::new();
    let first = env.tool("office.init", serde_json::json!({}));
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    assert_eq!(
        first["output"]["already_initialized"],
        serde_json::json!(false)
    );
    let library = env.library();
    let created = &first["output"]["created"];
    for name in BUNDLED_TEMPLATES {
        let relative = format!(".office/templates/{name}");
        assert_eq!(
            std::fs::read(library.join(&relative)).expect("installed template"),
            bundled_template(name),
            "{name}"
        );
        assert!(
            created
                .as_array()
                .expect("created")
                .iter()
                .any(|item| item == &relative),
            "{relative} missing from {created}"
        );
    }
    assert!(markdown_under(&library.join("knowledge")).is_empty());
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    let tree = git(&library, &["ls-tree", "-r", "--name-only", "HEAD"]);
    for name in BUNDLED_TEMPLATES {
        assert!(
            tree.lines()
                .any(|line| line == format!(".office/templates/{name}")),
            "{tree}"
        );
    }
    assert!(
        !tree
            .lines()
            .any(|line| line.starts_with("knowledge/") && line.ends_with(".md"))
    );
}

#[test]
fn init_preserves_an_edited_template_and_restores_only_the_deleted_one() {
    let env = Env::new();
    let first = env.tool("office.init", serde_json::json!({}));
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    let library = env.library();
    let user = library.join(".office/templates/user.md");
    let person = library.join(".office/templates/person.md");
    let original_person = std::fs::read(&person).expect("person bytes");
    let mut edited = std::fs::read(&user).expect("user bytes");
    edited.extend_from_slice(b"\nEdited by the user.\n");
    std::fs::write(&user, &edited).expect("edit template");
    std::fs::remove_file(&person).expect("delete template");

    let second = env.tool("office.init", serde_json::json!({}));
    assert_eq!(second["ok"], serde_json::json!(true), "{second}");
    assert_eq!(
        second["output"]["already_initialized"],
        serde_json::json!(false)
    );
    assert_eq!(
        second["output"]["created"],
        serde_json::json!([".office/templates/person.md"])
    );
    assert_eq!(std::fs::read(&user).expect("edited bytes"), edited);
    assert_eq!(
        std::fs::read(&person).expect("restored bytes"),
        original_person
    );
    // Restoring bytes that still match HEAD does not add a commit.
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "1");
    let dirty = git(&library, &["status", "--porcelain"]);
    assert!(
        dirty
            .lines()
            .any(|line| line.ends_with(".office/templates/user.md")),
        "{dirty}"
    );
    assert!(
        !dirty.lines().any(|line| line.contains("person.md")),
        "{dirty}"
    );

    std::fs::remove_file(&person).expect("delete committed template");
    git(&library, &["add", "--", ".office/templates/person.md"]);
    git(
        &library,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--quiet",
            "-m",
            "remove person template",
            "--",
            ".office/templates/person.md",
        ],
    );
    let third = env.tool("office.init", serde_json::json!({}));
    assert_eq!(third["ok"], serde_json::json!(true), "{third}");
    assert_eq!(
        third["output"]["already_initialized"],
        serde_json::json!(false)
    );
    assert_eq!(
        third["output"]["created"],
        serde_json::json!([".office/templates/person.md"])
    );
    assert_eq!(std::fs::read(&user).expect("edited bytes"), edited);
    assert_eq!(
        std::fs::read(&person).expect("restored bytes"),
        original_person
    );
    assert_eq!(git(&library, &["rev-list", "--count", "HEAD"]).trim(), "3");
    assert_eq!(
        git(
            &library,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        )
        .trim(),
        ".office/templates/person.md"
    );
    let still_dirty = git(&library, &["status", "--porcelain"]);
    assert!(
        still_dirty
            .lines()
            .any(|line| line.ends_with(".office/templates/user.md")),
        "{still_dirty}"
    );
    assert!(
        !still_dirty.lines().any(|line| line.contains("person.md")),
        "{still_dirty}"
    );
    assert!(markdown_under(&library.join("knowledge")).is_empty());
}
