//! Through the built binary: `office.init` scaffolds a Library idempotently,
//! `office.reindex` rebuilds a deleted index, `office.status` reports
//! Library health, and `office.knowledge-write`/`office.knowledge-show`
//! validate, commit and read pages (STD-04: temp roots, no real host state).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_office");

struct Env {
    _dir: tempfile::TempDir,
    workspace: PathBuf,
    state: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = dir.path().join("plugin_state");
        Self {
            _dir: dir,
            workspace,
            state,
        }
    }

    fn library(&self) -> PathBuf {
        self.workspace.join("library")
    }

    /// Run the binary with one envelope; `path` replaces the inherited PATH.
    fn tool_with_path(
        &self,
        tool: &str,
        input: serde_json::Value,
        path: &str,
    ) -> serde_json::Value {
        let request = serde_json::json!({
            "schema_version": 1,
            "tool": tool,
            "input": input,
            "context": {
                "workspace_root": self.workspace,
                "plugin_state": self.state,
            }
        })
        .to_string();
        let mut child = Command::new(BIN)
            .args(["orbit-tool"])
            .env_clear()
            .env("HOME", self.workspace.join("home"))
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(request.as_bytes())
            .expect("write request");
        let out = child.wait_with_output().expect("wait");
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("one JSON document")
    }

    fn tool(&self, tool: &str, input: serde_json::Value) -> serde_json::Value {
        let path = std::env::var("PATH").expect("PATH");
        self.tool_with_path(tool, input, &path)
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("git utf8")
}

const SOURCE: &str = "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\nsummary: Renewal pricing.\n---\n\n[00:12:31] Jane: searchable quixotic term here.\n";
const DOMAIN: &str = "---\nid: dom-acme\ntitle: Acme\n---\n\nAcme quixotic page.\n";

#[test]
fn config_set_updates_one_setting_and_commits_only_config() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", serde_json::json!({}))["ok"], true);
    let library = env.library();
    std::fs::write(library.join("unrelated.md"), "keep staged").unwrap();
    git(&library, &["add", "--", "unrelated.md"]);
    for (key, value) in [
        ("retention.meeting", serde_json::json!(30)),
        ("sensitivity.note", serde_json::json!("private")),
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
    let env = Env::new();
    assert_eq!(env.tool("office.init", serde_json::json!({}))["ok"], true);
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
            serde_json::json!({"key": "baseten_token", "value": "secret-fixture-value"}),
            "unknown_library_setting",
        ),
        (
            serde_json::json!({"key": "retention.meeting", "value": -1}),
            "invalid_library_setting",
        ),
        (
            serde_json::json!({"key": "sensitivity.note", "value": "secret-fixture-value"}),
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
    let env = Env::new();
    assert_eq!(env.tool("office.init", serde_json::json!({}))["ok"], true);
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
        "original.*\n"
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
    std::fs::write(library.join(".gitignore"), "custom-ignore\n").unwrap();
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
        "custom-ignore\n"
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

const BUNDLED_TEMPLATES: &[&str] = &[
    "client.md",
    "glossary.md",
    "goals.md",
    "person.md",
    "project.md",
    "rhythm.md",
    "runbook.md",
    "standing-directions.md",
    "systems.md",
    "topic.md",
    "user.md",
];

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

#[test]
fn status_counts_off_template_pages_and_names_a_broken_template() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let quiet = env.tool("office.status", serde_json::json!({}));
    assert_eq!(quiet["ok"], serde_json::json!(true), "{quiet}");
    assert_eq!(
        quiet["output"]["library"]["pages_off_template"],
        serde_json::json!(0)
    );
    assert_eq!(
        quiet["output"]["library"]["template_invalid"],
        serde_json::json!([])
    );

    let library = env.library();
    std::fs::write(
        library.join("knowledge/domain/ada.md"),
        "---\nid: dom-person-ada\ntype: domain\ntemplate: person\ntitle: Ada\n---\n\n## Role\nKeeps the notes.\n",
    )
    .unwrap();
    std::fs::write(
        library.join("knowledge/domain/bev.md"),
        "---\nid: dom-person-bev\ntype: domain\ntemplate: person\ntitle: Bev\n---\n\n## History\nNo role.\n",
    )
    .unwrap();
    std::fs::write(
        library.join("knowledge/domain/free.md"),
        "---\nid: dom-free\ntype: domain\ntitle: Free\n---\n\n## Role\nUnbound.\n",
    )
    .unwrap();
    std::fs::write(
        library.join(".office/templates/notes.md"),
        "not a template\n",
    )
    .unwrap();
    let reindex = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(
        reindex["output"]["indexed_documents"],
        serde_json::json!(3),
        "{reindex}"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(true), "{status}");
    assert!(status.get("error").is_none());
    assert_eq!(
        status["output"]["library"]["initialized"],
        serde_json::json!(true)
    );
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(3)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["domain"],
        serde_json::json!(3)
    );
    assert_eq!(
        status["output"]["library"]["pages_off_template"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["library"]["template_invalid"],
        serde_json::json!([".office/templates/notes.md"])
    );
    assert_eq!(
        status["output"]["secrets"]["meta_api_key"],
        serde_json::json!("unset")
    );
    assert_eq!(status["output"]["version"], serde_json::json!("0.1.0"));
}

#[test]
fn status_reports_an_invalid_config_as_its_typed_error_not_as_uninitialized() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    std::fs::write(env.library().join(".office/config.yaml"), "bogus: true\n").unwrap();

    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(false), "{status}");
    assert_eq!(
        status["error"]["code"],
        serde_json::json!("invalid_library_setting"),
        "{status}"
    );
    assert!(
        status["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("bogus")),
        "{status}"
    );
    assert!(status.get("output").is_none(), "{status}");
}

#[test]
fn status_on_an_absent_library_reports_uninitialized_without_error_or_state() {
    let env = Env::new();
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(true), "{status}");
    assert!(status.get("error").is_none(), "{status}");
    assert_eq!(
        status["output"]["library"]["initialized"],
        serde_json::json!(false)
    );
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(0)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["source"],
        serde_json::json!(0)
    );
    assert!(!env.library().exists(), "status created the Library root");
}

#[test]
fn status_reports_an_unreplayable_write_journal_as_its_typed_error_and_keeps_it() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    // A journal from a newer backend version is never replayed: status refuses
    // with its typed error and leaves the pending decision in place.
    let journal = env.state.join("library.pending-write.json");
    let text = serde_json::json!({
        "version": 2,
        "root": env.library().canonicalize().expect("Library root"),
        "relative": "knowledge/domain/recovered.md",
        "text": DOMAIN,
        "token": "123-456"
    })
    .to_string();
    std::fs::write(&journal, &text).expect("interrupted write decision");

    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(false), "{status}");
    assert_eq!(
        status["error"]["code"],
        serde_json::json!("library_journal_invalid"),
        "{status}"
    );
    assert!(status.get("output").is_none(), "{status}");
    assert_eq!(
        std::fs::read_to_string(&journal).expect("kept journal"),
        text
    );
}

#[test]
fn capture_and_source_show_through_the_binary() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");

    // Pasted text dedupes to one id.
    let first = env.tool(
        "office.capture",
        serde_json::json!({"kind": "note", "title": "Binary note", "text": "Captured through the binary."}),
    );
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    let second = env.tool(
        "office.capture",
        serde_json::json!({"kind": "note", "title": "Binary note", "text": "Captured through the binary."}),
    );
    assert_eq!(second["output"]["id"], first["output"]["id"], "{second}");
    assert_eq!(second["output"]["deduplicated"], serde_json::json!(true));

    // A video upload with a VTT transcript needs no model call.
    let video = env.workspace.join("talk.mp4");
    std::fs::write(&video, b"video-bytes").expect("video");
    let vtt = env.workspace.join("talk.vtt");
    std::fs::write(
        &vtt,
        "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\n<v Jane>Opening line.</v>\n",
    )
    .expect("vtt");
    let captured = env.tool(
        "office.capture",
        serde_json::json!({
            "upload_path": video.to_string_lossy(),
            "transcript_path": vtt.to_string_lossy(),
            "title": "Binary KT",
        }),
    );
    assert_eq!(
        captured["output"]["kind"],
        serde_json::json!("video"),
        "{captured}"
    );
    let shown = env.tool(
        "office.source-show",
        serde_json::json!({"id": captured["output"]["id"], "anchor": "#t=00:00:01"}),
    );
    assert!(
        shown["output"]["body"]
            .as_str()
            .expect("body")
            .contains("Jane"),
        "{shown}"
    );
    let bad_anchor = env.tool(
        "office.source-show",
        serde_json::json!({"id": captured["output"]["id"], "anchor": "#L42"}),
    );
    assert_eq!(
        bad_anchor["error"]["code"],
        serde_json::json!("anchor_out_of_range"),
        "{bad_anchor}"
    );

    // A symlink escaping the workspace is refused.
    let outside = env._dir.path().join("outside.bin");
    std::fs::write(&outside, b"outside").expect("outside");
    let link = env.workspace.join("escape.bin");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    let refused = env.tool(
        "office.capture",
        serde_json::json!({"upload_path": link.to_string_lossy(), "title": "Escape"}),
    );
    assert_eq!(
        refused["error"]["code"],
        serde_json::json!("upload_outside_allowed_roots"),
        "{refused}"
    );

    // Conversation appends preserve earlier lines.
    let opened = env.tool(
        "office.capture",
        serde_json::json!({"kind": "conversation", "title": "Binary session", "entries": [{"kind": "fact", "text": "Fact one."}]}),
    );
    let id = opened["output"]["id"].as_str().expect("id").to_string();
    let appended = env.tool(
        "office.capture",
        serde_json::json!({"append_to": id, "entries": [{"kind": "handoff", "text": "Next step."}]}),
    );
    assert_eq!(
        appended["output"]["appended"],
        serde_json::json!(1),
        "{appended}"
    );
    let log = env.tool("office.source-show", serde_json::json!({"id": id}));
    let body = log["output"]["body"].as_str().expect("body").to_string();
    assert!(body.contains("fact: Fact one."), "{body}");
    assert!(body.contains("handoff: Next step."), "{body}");
}

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

const RENEWAL_PAGE: &str = "---\nid: dom-project-acme-renewal\ntype: domain\ntemplate: project\ntitle: Acme renewal\naliases: [the renewal]\ncites:\n  - src-20261028-acme-qbr#t=00:12:31\nrefs:\n  - { label: Renewal tickets, account: \"jira:ACME\" }\n  - { label: Jane Park, contact: jane@acme.example }\n---\n## Status\nRenewal is due in March [src-20261028-acme-qbr#t=00:10:00].\n\n## Background\nPrice sensitivity is high [src-20261028-acme-qbr#t=00:12:31].\n";

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
