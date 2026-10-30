//! `office.library-sync` against a real bare repository on a `file` remote:
//! through the binary for push, pull and git isolation, and through public
//! dispatch with an offline Orbit for incoming updates and conflicts.
use super::{Env, output};
use office_core::domain::source::sha256_hex;
use office_core::ports::*;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

const PAGE: &str = "knowledge/domain/dom-onboarding.md";
const SOURCE_DIR: &str = "sources/meeting/2026/10/src-20261028-kickoff";

fn own_page(body: &str) -> String {
    format!(
        "---\nid: dom-onboarding\ntitle: Onboarding\nshareable: true\ncites: [\"src-20261028-kickoff#t=00:00:05\"]\nrefs:\n  - {{label: Owner, contact: owner@example.invalid}}\n  - {{label: Notes, path: /home/member/notes}}\n---\n## Overview\n{body} [src-20261028-kickoff#t=00:00:05].\n"
    )
}

/// A bare team repository and a colleague's working clone of it.
struct Team {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Team {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::write(
            root.join("home/.gitconfig"),
            "[user]\n\tname = Colleague\n\temail = colleague@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let team = Self { _dir: dir, root };
        team.git(&team.root, &["init", "--quiet", "--bare", "team.git"]);
        team
    }

    fn remote(&self) -> String {
        self.root.join("team.git").to_string_lossy().into_owned()
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("PATH", std::env::var_os("PATH").unwrap())
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn files(&self) -> Vec<String> {
        self.git(
            &self.root.join("team.git"),
            &["ls-tree", "-r", "--name-only", "main"],
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    fn commits(&self) -> usize {
        self.git(
            &self.root.join("team.git"),
            &["rev-list", "--count", "main"],
        )
        .trim()
        .parse()
        .unwrap()
    }

    fn show(&self, path: &str) -> String {
        self.git(
            &self.root.join("team.git"),
            &["show", &format!("main:{path}")],
        )
    }

    /// The colleague commits `edit` on top of the current branch.
    fn commit(&self, edit: impl FnOnce(&Path)) {
        let clone = self.root.join("colleague");
        if clone.exists() {
            self.git(&clone, &["fetch", "--quiet", "origin"]);
            self.git(&clone, &["reset", "--quiet", "--hard", "origin/main"]);
        } else {
            self.git(
                &self.root,
                &["clone", "--quiet", &self.remote(), "colleague"],
            );
        }
        edit(&clone);
        self.git(&clone, &["add", "-A"]);
        self.git(&clone, &["commit", "--quiet", "-m", "colleague edit"]);
        self.git(&clone, &["push", "--quiet", "origin", "HEAD:main"]);
    }

    /// The colleague rewrites the shared page body.
    fn edit_page(&self, from: &str, to: &str) {
        self.commit(|clone| {
            let path = clone.join(PAGE);
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains(from), "{text}");
            let next = text
                .replace(from, to)
                .replace("shared_by: \"alice\"", "shared_by: \"bob\"");
            std::fs::write(path, next).unwrap();
        });
    }
}

/// An initialized workspace sharing as `member`, with a git identity.
fn member(team: &Team, member: &str) -> Env {
    member_in(Env::new(), team, member)
}

/// An initialized workspace in `env` sharing as `member`, with a git identity.
fn member_in(env: Env, team: &Team, member: &str) -> Env {
    std::fs::create_dir_all(env.workspace.join("home")).unwrap();
    std::fs::write(
        env.workspace.join("home/.gitconfig"),
        format!("[user]\n\tname = {member}\n\temail = {member}@example.invalid\n"),
    )
    .unwrap();
    output(&env, "office.init", json!({}));
    output(
        &env,
        "office.config-set",
        json!({"key":"shared","value":{"remote":team.remote(),"branch":"main","member":member}}),
    );
    env
}

fn store(env: &Env) -> office_store::LibraryStore {
    office_store::LibraryStore::open(&env.library(), &env.state).unwrap()
}

/// Alice's shareable page, source and sidecar, plus an unshared page.
fn alice_documents(env: &Env) {
    let store = store(env);
    store
        .write_document(
            Path::new(&format!("{SOURCE_DIR}/source.md")),
            "---\nid: src-20261028-kickoff\nkind: meeting\ntitle: Kickoff\nshareable: true\n---\n[00:00:01] Alice: Welcome.\n[00:00:09] Alice: Read the onboarding page.\n",
        )
        .unwrap();
    std::fs::write(
        env.library().join(SOURCE_DIR).join("frames.md"),
        "[00:00:01] Welcome slide [frame](#frame=00:00:01)\n",
    )
    .unwrap();
    store
        .write_document(Path::new(PAGE), &own_page("Start with the kickoff"))
        .unwrap();
    store
        .write_document(
            Path::new("knowledge/domain/dom-private-notes.md"),
            "---\nid: dom-private-notes\ntitle: Notes\n---\n## Overview\nNot shared.\n",
        )
        .unwrap();
}

/// Run the binary with `extra` on top of the clean test environment.
fn run(env: &Env, tool: &str, input: Value, extra: &[(&str, OsString)]) -> Value {
    let request = json!({
        "schema_version": 1,
        "tool": tool,
        "input": input,
        "context": {"workspace_root": env.workspace, "plugin_state": env.state},
    })
    .to_string();
    let mut command = Command::new(crate::BIN);
    command
        .arg("orbit-tool")
        .env_clear()
        .env("HOME", env.workspace.join("home"))
        .env("PATH", std::env::var_os("PATH").unwrap());
    for (name, value) in extra {
        command.env(name, value);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(request.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[cfg(unix)]
#[test]
fn two_workspaces_push_and_pull_with_isolated_git_and_skip_invalid_files() {
    use std::os::unix::fs::PermissionsExt;
    let team = Team::new();
    let alice = member(&team, "alice");
    alice_documents(&alice);
    // Ambient git variables and a hook must not reach or run in git children.
    let marker = alice.workspace.join("hook-ran");
    let hooks = alice.workspace.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for name in ["pre-push", "post-checkout", "reference-transaction"] {
        let path = hooks.join(name);
        std::fs::write(&path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let poisoned = [
        (
            "GIT_DIR",
            alice.workspace.join("elsewhere").into_os_string(),
        ),
        ("GIT_WORK_TREE", alice.workspace.clone().into_os_string()),
        ("GIT_CONFIG_COUNT", "1".into()),
        ("GIT_CONFIG_KEY_0", "core.hooksPath".into()),
        ("GIT_CONFIG_VALUE_0", hooks.clone().into_os_string()),
        ("GIT_AUTHOR_NAME", "Ambient Author".into()),
        ("GIT_TERMINAL_PROMPT", "1".into()),
    ];
    let first = run(&alice, "office.library-sync", json!({}), &poisoned);
    assert_eq!(first["ok"], true, "{first}");
    let source = format!("members/alice/{SOURCE_DIR}");
    assert_eq!(
        first["output"]["pushed"],
        json!([
            PAGE,
            format!("{source}/frames.md"),
            format!("{source}/source.md")
        ]),
        "{first}"
    );
    assert_eq!(
        team.files(),
        [
            ".office-shared.yaml".to_string(),
            PAGE.into(),
            format!("{source}/frames.md"),
            format!("{source}/source.md"),
        ]
    );
    assert_eq!(team.show(".office-shared.yaml"), "format: 1\n");
    let shared = team.show(PAGE);
    assert!(shared.contains("shared_by: \"alice\""), "{shared}");
    assert!(shared.contains("alice/src-20261028-kickoff#t=00:00:05"));
    assert!(!shared.contains("owner@example.invalid") && !shared.contains("/home/member"));
    assert_eq!(
        team.git(
            &team.root.join("team.git"),
            &["log", "-1", "--format=%an|%s", "main"]
        ),
        "alice|alice: share 2 documents, remove 0\n"
    );
    // A hook installed in the clone after the first sync stays inert.
    let clone_hooks = alice.state.join("shared/repo/.git/hooks");
    std::fs::create_dir_all(&clone_hooks).unwrap();
    std::fs::copy(hooks.join("pre-push"), clone_hooks.join("pre-push")).unwrap();
    let second = run(&alice, "office.library-sync", json!({}), &poisoned);
    assert_eq!(second["output"]["pushed"], json!([]), "{second}");
    assert_eq!(second["output"]["pulled"], json!([]));
    assert_eq!(team.commits(), 1);
    assert!(!marker.exists(), "a git hook ran");
    let state = alice.state.join("shared/state.json");
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o600
    );

    team.commit(|clone| {
        std::os::unix::fs::symlink("/etc/passwd", clone.join("knowledge/domain/dom-link.md"))
            .unwrap();
        std::fs::create_dir_all(clone.join("notes")).unwrap();
        std::fs::write(clone.join("notes/outside.md"), "Outside the layout.\n").unwrap();
        let private = clone.join("members/bob/sources/2026/10/src-20261028-private");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(
            private.join("source.md"),
            "---\nid: src-20261028-private\nkind: note\ntitle: Private\nsensitivity: private\nshared_by: bob\n---\nSecret.\n",
        )
        .unwrap();
    });
    let joiner = member(&team, "joiner");
    let pulled = run(&joiner, "office.library-sync", json!({}), &[]);
    assert_eq!(pulled["ok"], true, "{pulled}");
    let pulled_output = &pulled["output"];
    assert_eq!(pulled_output["pushed"], json!([]));
    assert_eq!(
        pulled_output["pulled"],
        json!([
            PAGE,
            format!("{source}/frames.md"),
            format!("{source}/source.md")
        ])
    );
    let skipped: Vec<(String, String)> = pulled_output["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["path"].as_str().unwrap().to_string(),
                entry["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        skipped,
        [
            (
                "knowledge/domain/dom-link.md".to_string(),
                "shared_file_linked".to_string()
            ),
            (
                "members/bob/sources/2026/10/src-20261028-private/source.md".into(),
                "share_private_refused".into()
            ),
            (
                "notes/outside.md".into(),
                "shared_path_outside_layout".into()
            ),
        ],
        "{pulled_output}"
    );
    let hits = output_of(
        &joiner,
        "office.search",
        json!({"query":"onboarding","layer":"shared"}),
    );
    assert_eq!(hits["hits"][0]["id"], "dom-onboarding", "{hits}");
    assert_eq!(hits["hits"][0]["shared_by"], "alice");
    let secret = output_of(
        &joiner,
        "office.search",
        json!({"query":"secret","layer":"shared"}),
    );
    assert_eq!(secret["count"], 0, "{secret}");
    let shown = output_of(
        &joiner,
        "office.source-show",
        json!({"id":"alice/src-20261028-kickoff"}),
    );
    assert_eq!(shown["shared_by"], "alice");
    // A dry run on top changes nothing and an unreachable remote names no path.
    let before = std::fs::read(joiner.state.join("shared/state.json")).unwrap();
    let dry = run(&joiner, "office.library-sync", json!({"dry_run":true}), &[]);
    assert_eq!(dry["output"]["synced_at"], Value::Null, "{dry}");
    assert_eq!(
        std::fs::read(joiner.state.join("shared/state.json")).unwrap(),
        before
    );
    output(
        &joiner,
        "office.config-set",
        json!({"key":"shared","value":{"remote":team.root.join("gone.git"),"branch":"main","member":"joiner"}}),
    );
    let unreachable = run(&joiner, "office.library-sync", json!({}), &[]);
    assert_eq!(unreachable["error"]["code"], "shared_remote_unreachable");
    let message = unreachable["error"]["message"].as_str().unwrap();
    assert!(
        !message.contains("gone.git") && !message.contains("fatal"),
        "{message}"
    );
}

/// A symlinked ancestor above plugin state is host infrastructure (on
/// macOS `/var` -> `/private/var`, with `$TMPDIR` beneath it), not a state
/// symlink: sync resolves through it instead of refusing.
#[cfg(unix)]
#[test]
fn sync_works_when_a_state_ancestor_is_a_symlink() {
    let team = Team::new();
    let mut env = Env::new();
    let root = env.workspace.parent().unwrap().to_path_buf();
    std::os::unix::fs::symlink(&root, root.join("state-alias")).unwrap();
    env.state = root.join("state-alias").join("plugin_state");
    let alice = member_in(env, &team, "alice");
    alice_documents(&alice);
    let first = run(&alice, "office.library-sync", json!({}), &[]);
    assert_eq!(first["ok"], true, "{first}");
    let source = format!("members/alice/{SOURCE_DIR}");
    assert_eq!(
        first["output"]["pushed"],
        json!([
            PAGE,
            format!("{source}/frames.md"),
            format!("{source}/source.md")
        ]),
        "{first}"
    );
}

/// Plugin state itself reached through a symlink is refused (STD-05 R9).
#[cfg(unix)]
#[test]
fn sync_refuses_when_plugin_state_itself_is_a_symlink() {
    let team = Team::new();
    let env = Env::new();
    let real_state = env.workspace.parent().unwrap().join("real-plugin-state");
    std::fs::create_dir_all(&real_state).unwrap();
    std::os::unix::fs::symlink(&real_state, &env.state).unwrap();
    let alice = member_in(env, &team, "alice");
    alice_documents(&alice);
    let refused = run(&alice, "office.library-sync", json!({}), &[]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["error"]["code"], "shared_storage_failed",
        "{refused}"
    );
}

/// A symlink below plugin state (here `shared/`) is refused (STD-05 R9).
#[cfg(unix)]
#[test]
fn sync_refuses_when_a_component_below_plugin_state_is_a_symlink() {
    let team = Team::new();
    let alice = member(&team, "alice");
    alice_documents(&alice);
    let first = run(&alice, "office.library-sync", json!({}), &[]);
    assert_eq!(first["ok"], true, "{first}");
    let shared = alice.state.join("shared");
    std::fs::remove_dir_all(&shared).unwrap();
    std::os::unix::fs::symlink("/etc", &shared).unwrap();
    let refused = run(&alice, "office.library-sync", json!({}), &[]);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["error"]["code"], "shared_storage_failed",
        "{refused}"
    );
}

fn output_of(env: &Env, tool: &str, input: Value) -> Value {
    let response = run(env, tool, input, &[]);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

/// Offline Orbit that files, lists, shows and closes proposals.
#[derive(Clone, Default)]
struct Proposals {
    tasks: Rc<RefCell<Vec<Value>>>,
    artifacts: Rc<RefCell<std::collections::BTreeMap<String, String>>>,
}

impl Proposals {
    fn added(&self) -> usize {
        self.tasks.borrow().len()
    }
    fn payload(&self, id: &str) -> (Value, String) {
        let text = self.artifacts.borrow()[id].clone();
        (
            serde_json::from_str(&text).unwrap(),
            sha256_hex(text.as_bytes()),
        )
    }
}

impl Orbit for Proposals {
    fn task_list(&self, input: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        let tag = input["tag"].clone();
        let tasks: Vec<Value> = self
            .tasks
            .borrow()
            .iter()
            .filter(|task| task["tags"].as_array().unwrap().contains(&tag))
            .filter(|task| {
                input
                    .get("status")
                    .is_none_or(|status| task["status"] == *status)
            })
            .cloned()
            .collect();
        Ok(json!({"total":tasks.len(),"tasks":tasks,"truncated":false}))
    }
    fn task_add(&self, input: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        let id = format!("fixture-{}", self.added() + 1);
        let task = json!({"id":id,"status":"proposed","tags":input["tags"],"title":input["title"],"updated_at":"2026-10-28T00:00:00Z"});
        self.tasks.borrow_mut().push(task.clone());
        Ok(task)
    }
    fn task_show(&self, input: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        Ok(self
            .tasks
            .borrow()
            .iter()
            .find(|task| task["id"] == input["id"])
            .cloned()
            .unwrap())
    }
    fn task_update(&self, input: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        let mut tasks = self.tasks.borrow_mut();
        let task = tasks
            .iter_mut()
            .find(|task| task["id"] == input["id"])
            .unwrap();
        task["status"] = input["status"].clone();
        Ok(task.clone())
    }
    fn task_artifact_put(
        &self,
        input: &serde_json::Map<String, Value>,
    ) -> office_core::Result<Value> {
        let text = std::fs::read_to_string(input["source_path"].as_str().unwrap()).unwrap();
        self.artifacts
            .borrow_mut()
            .insert(input["id"].as_str().unwrap().into(), text);
        Ok(json!({}))
    }
    fn task_artifact_get(
        &self,
        input: &serde_json::Map<String, Value>,
    ) -> office_core::Result<Value> {
        let text = self.artifacts.borrow()[input["id"].as_str().unwrap()].clone();
        Ok(json!({"content":text}))
    }
}

fn ports(env: &Env, proposals: Proposals) -> Ports<'static> {
    let mut ports = super::ports(env, super::Callbacks::default());
    ports.orbit = Box::new(proposals);
    ports.shared = Box::new(office_store::SharedStore::new(
        env.state.clone(),
        office_store::git_environment(|name| match name {
            "PATH" => std::env::var_os("PATH"),
            "HOME" => Some(env.workspace.join("home").into_os_string()),
            _ => None,
        }),
    ));
    ports
}

fn sync(ports: &Ports<'_>) -> Value {
    super::dispatched(ports, "office.library-sync", json!({}))
}

fn accept(ports: &Ports<'_>, proposals: &Proposals, id: &str, choice: Option<&str>) -> Value {
    let (_, digest) = proposals.payload(id);
    let mut input = json!({"id":id,"sha256":digest});
    if let Some(choice) = choice {
        input["choice"] = json!(choice);
    }
    super::dispatch(ports, "office.proposal-accept", input)
}

#[test]
fn incoming_updates_conflicts_and_unsharing_round_trip_through_the_remote() {
    let team = Team::new();
    let alice = member(&team, "alice");
    alice_documents(&alice);
    let proposals = Proposals::default();
    let ports = ports(&alice, proposals.clone());
    let read_own = || std::fs::read_to_string(alice.library().join(PAGE)).unwrap();
    sync(&ports);
    assert_eq!(team.commits(), 1);

    // A colleague improves the page Alice holds unchanged.
    team.edit_page("Start with the kickoff", "Begin with the kickoff");
    let result = sync(&ports);
    assert_eq!(
        result["incoming"],
        json!([{"page":"dom-onboarding","proposal":"fixture-1","created":true}]),
        "{result}"
    );
    assert_eq!(result["pushed"], json!([]));
    let again = sync(&ports);
    assert_eq!(again["incoming"][0]["created"], false, "{again}");
    assert_eq!(proposals.added(), 1);
    let (payload, _) = proposals.payload("fixture-1");
    assert_eq!(payload["kind"], "knowledge_update");
    let refused = accept(&ports, &proposals, "fixture-1", Some("theirs"));
    assert_eq!(refused["error"]["code"], "invalid_argument", "{refused}");
    let accepted = accept(&ports, &proposals, "fixture-1", None);
    assert_eq!(accepted["ok"], true, "{accepted}");
    let own = read_own();
    assert!(
        own.contains("Begin with the kickoff [src-20261028-kickoff#t=00:00:05]"),
        "{own}"
    );
    assert!(own.contains("owner@example.invalid") && own.contains("/home/member/notes"));
    assert!(!own.contains("shared_by"));
    let settled = sync(&ports);
    assert_eq!(settled["pushed"], json!([]), "{settled}");
    assert_eq!(settled["incoming"], json!([]));
    assert_eq!(team.commits(), 2);

    // Both sides change: a conflict, nothing pushed; Use theirs.
    team.edit_page("Begin with the kickoff", "Their kickoff guidance");
    store(&alice)
        .write_document(Path::new(PAGE), &own_page("My kickoff guidance"))
        .unwrap();
    let conflict = sync(&ports);
    assert_eq!(
        conflict["conflicts"],
        json!([{"page":"dom-onboarding","proposal":"fixture-2","created":true}]),
        "{conflict}"
    );
    assert_eq!(conflict["pushed"], json!([]));
    assert_eq!(sync(&ports)["conflicts"][0]["created"], false);
    assert_eq!(proposals.added(), 2);
    assert_eq!(team.commits(), 3);
    let missing = accept(&ports, &proposals, "fixture-2", None);
    assert_eq!(missing["error"]["code"], "invalid_argument", "{missing}");
    let theirs = accept(&ports, &proposals, "fixture-2", Some("theirs"));
    assert_eq!(theirs["ok"], true, "{theirs}");
    let own = read_own();
    assert!(
        own.contains("Their kickoff guidance [src-20261028-kickoff#t=00:00:05]"),
        "{own}"
    );
    assert!(own.contains("owner@example.invalid"));
    assert_eq!(sync(&ports)["pushed"], json!([]));
    assert_eq!(team.commits(), 3);

    // Both sides change again; Keep mine pushes Alice's copy next sync.
    team.edit_page("Their kickoff guidance", "Their second pass");
    store(&alice)
        .write_document(Path::new(PAGE), &own_page("My final guidance"))
        .unwrap();
    assert_eq!(sync(&ports)["conflicts"][0]["proposal"], "fixture-3");
    let mine = accept(&ports, &proposals, "fixture-3", Some("mine"));
    assert_eq!(mine["ok"], true, "{mine}");
    assert_eq!(read_own(), own_page("My final guidance"));
    let pushed = sync(&ports);
    assert_eq!(pushed["pushed"], json!([PAGE]), "{pushed}");
    let shared = team.show(PAGE);
    assert!(shared.contains("My final guidance") && shared.contains("shared_by: \"alice\""));
    assert_eq!(team.commits(), 5);

    // Unsharing removes Alice's paths on the next push.
    super::dispatched(
        &ports,
        "office.share-set",
        json!({"id":"dom-onboarding","shareable":false}),
    );
    let removed = sync(&ports);
    assert_eq!(removed["removed"], json!([PAGE]), "{removed}");
    assert!(!team.files().contains(&PAGE.to_string()));
}
