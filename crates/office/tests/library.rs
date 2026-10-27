//! Through the built binary: `office.init` scaffolds a Library idempotently,
//! `office.reindex` rebuilds a deleted index, and `office.status` reports
//! Library health (STD-04: temp roots, no real host state).
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
