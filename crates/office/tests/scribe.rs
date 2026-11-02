//! `office.scribe-pending` and `office.scribe-append` through the built
//! binary against a temporary workspace (STD-04): the private message file
//! under the Library's ignored `.office/state/scribe/`, inside the plugin's
//! write grant, and the SCR file committed to the Library, renamed in one
//! commit.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SESSION: &str = "fixture-session";

struct Fixture {
    root: tempfile::TempDir,
    library_dir: &'static str,
}

impl Fixture {
    fn new() -> Self {
        Self::with_library_dir("library")
    }

    /// A workspace whose plugin config sets `library_dir`.
    fn with_library_dir(library_dir: &'static str) -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            library_dir,
        };
        std::fs::create_dir_all(fixture.workspace()).unwrap();
        assert_eq!(fixture.call("office.init", json!({}))["ok"], true);
        fixture
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("workspace")
    }

    fn library(&self) -> PathBuf {
        self.workspace().join(self.library_dir)
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    fn call(&self, tool: &str, input: Value) -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("PATH", std::env::var("PATH").unwrap())
            .env("TZ", "UTC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"schema_version":1,"tool":tool,"input":input,"context":{
            "workspace_root":self.workspace(),"plugin_state":self.state(),
            "config":{"library_dir":self.library_dir}}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn ok(&self, tool: &str, input: Value) -> Value {
        let response = self.call(tool, input);
        assert_eq!(response["ok"], true, "{response}");
        response["output"].clone()
    }

    fn transcript(&self, messages: usize) -> PathBuf {
        let texts = [
            ("user", "Plan the scribe v1 rollout.\nKeep it model-free."),
            ("assistant", "Merged as 7266911."),
            ("user", "Decision: ship parse and append first."),
            ("assistant", "Waiting on CI."),
        ];
        let text: Vec<String> = texts[..messages]
            .iter()
            .enumerate()
            .map(|(n, (role, text))| {
                json!({"type":role,"sessionId":SESSION,"timestamp":format!("2026-11-01T03:0{n}:00Z"),
                    "cwd":"/work/constellation","message":{"role":role,"content":text}})
                .to_string()
            })
            .collect();
        let path = self.workspace().join("session.jsonl");
        std::fs::write(&path, text.join("\n") + "\n").unwrap();
        path
    }

    fn pending(&self, messages: usize) -> Value {
        let transcript = self.transcript(messages);
        self.pending_call(&transcript)
    }

    fn pending_call(&self, transcript: &Path) -> Value {
        self.ok(
            "office.scribe-pending",
            json!({"session_id":SESSION,"transcript":transcript,"provider":"claude-code","cwd":"/work/constellation"}),
        )
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.library())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", self.root.path().join("home"))
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }
}

/// Every path under `dir`, files and directories, without following links.
fn tree(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        out.push(path.clone());
        if std::fs::symlink_metadata(&path).unwrap().is_dir() {
            tree(&path, out);
        }
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn pending_writes_a_private_message_file_whose_ids_only_grow() {
    let f = Fixture::new();
    let first = f.pending(2);
    assert_eq!(
        first["parsed"],
        "library/.office/state/scribe/fixture-session.jsonl"
    );
    let file = f.workspace().join(first["parsed"].as_str().unwrap());
    let short = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        short,
        "{\"id\":\"m-0001\",\"role\":\"user\",\"at\":\"2026-11-01T03:00:00Z\",\"text\":\"Plan the scribe v1 rollout.\\nKeep it model-free.\"}\n{\"id\":\"m-0002\",\"role\":\"assistant\",\"at\":\"2026-11-01T03:01:00Z\",\"text\":\"Merged as 7266911.\"}\n"
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
        assert_eq!(
            mode(
                &f.library()
                    .join(".office/state/scribe/fixture-session.meta.json")
            ),
            0o600
        );
    }
    assert_eq!(
        first["scr"]["path"],
        "sources/conversation/2026/11/SCR-01-0300-plan-the-scribe-v1-rollout.md"
    );
    f.pending(4);
    let long = std::fs::read_to_string(&file).unwrap();
    assert!(long.starts_with(&short));
    assert_eq!(long.lines().count(), 4);
}

/// The installed plugin may write only under `{{workspace}}/library` and
/// `{{plugin_state}}`; Orbit refuses any grant under the workspace's
/// `.orbit`. Parsing a new session writes nothing else, under a configured
/// `library_dir`, and the Library's work tree stays clean.
#[test]
fn pending_writes_only_inside_the_library_and_plugin_state() {
    let f = Fixture::with_library_dir("library/office");
    let transcript = f.transcript(4);
    let mut before = Vec::new();
    tree(f.root.path(), &mut before);
    let out = f.pending_call(&transcript);
    let mut after = Vec::new();
    tree(f.root.path(), &mut after);
    let outside: Vec<&PathBuf> = after
        .iter()
        .filter(|path| !before.contains(path))
        .filter(|path| !path.starts_with(f.library()) && !path.starts_with(f.state()))
        .collect();
    assert_eq!(
        outside,
        Vec::<&PathBuf>::new(),
        "written outside the grants"
    );
    assert_eq!(
        out["parsed"],
        "library/office/.office/state/scribe/fixture-session.jsonl"
    );
    let parsed = f.workspace().join(out["parsed"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(parsed).unwrap().lines().count(), 4);
    assert_eq!(f.git(&["status", "--porcelain", "--ignored=no"]), "");

    // Append reads the parse back from the same place.
    let appended = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","entries":[]}),
    );
    assert_eq!(appended["scribed_through"], "m-0004");
    let path = appended["path"].as_str().unwrap();
    let text = std::fs::read_to_string(f.library().join(path)).unwrap();
    assert!(
        text.contains("\nparsed: \"library/office/.office/state/scribe/fixture-session.jsonl\"\n"),
        "{text}"
    );
}

#[test]
fn append_commits_the_scr_file_and_a_title_renames_it_in_one_commit() {
    let f = Fixture::new();
    f.pending(4);
    let out = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0003","entries":[
            {"msg":"m-0003","quote":"ship parse and append first","tag":"decision","topics":["constellation"]},
            {"msg":"m-0002","quote":"Merged as 7266911","tag":"decision","topics":[]},
            {"text":"CI for the scribe tools","tag":"pending","topics":[],"by":"opus"},
        ]}),
    );
    assert_eq!(out["refused"][0]["reason"], "tag_not_permitted");
    let path = "sources/conversation/2026/11/SCR-01-0300-plan-the-scribe-v1-rollout.md";
    let text = std::fs::read_to_string(f.library().join(path)).unwrap();
    let body: Vec<&str> = text.lines().skip(21).collect();
    assert_eq!(body.len(), 2);
    assert_eq!(
        body[0],
        "- m-0003 2026-11-01T03:02+00:00 #decision #no-why #constellation \"ship parse and append first\""
    );
    assert!(body[1].starts_with("- note "));
    assert!(body[1].ends_with(" #pending \"CI for the scribe tools\" by: opus"));
    assert!(text.contains("\nscribed_through: \"m-0003\"\n"));
    assert_eq!(f.git(&["status", "--porcelain", "--", "sources"]), "");
    assert_eq!(f.git(&["ls-files", "sources/conversation"]).trim(), path);

    // A re-run adds nothing and makes no commit.
    let head = f.git(&["rev-parse", "HEAD"]);
    let rerun = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0003","entries":[]}),
    );
    assert_eq!(rerun["written"], false);
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);

    let renamed = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","title":"Scribe v1 rollout","entries":[
            {"msg":"m-0004","quote":"Waiting on CI.","tag":"resolved","topics":[],"by":"opus",
             "resolves":"SCR-01-0300-plan-the-scribe-v1-rollout#L23"}]}),
    );
    assert_eq!(renamed["refused"], json!([]));
    assert_eq!(
        renamed["renamed_from"],
        "SCR-01-0300-plan-the-scribe-v1-rollout"
    );
    let new_path = "sources/conversation/2026/11/SCR-01-0300-scribe-v1-rollout.md";
    assert!(!f.library().join(path).exists());
    let text = std::fs::read_to_string(f.library().join(new_path)).unwrap();
    assert!(text.contains("\naliases: [\"SCR-01-0300-plan-the-scribe-v1-rollout\"]\n"));
    assert_eq!(
        f.git(&["ls-files", "sources/conversation"]).trim(),
        new_path
    );
    assert_eq!(f.git(&["status", "--porcelain"]), "");
    let changed = f.git(&["show", "--no-renames", "--name-status", "--format=", "HEAD"]);
    assert!(changed.contains(&format!("D\t{path}")), "{changed}");
    assert!(changed.contains(&format!("A\t{new_path}")), "{changed}");
}

/// The web Scribe screen's note: read the session's cursor from
/// `office.scribe-query`, then append one `by: user` note at that cursor.
#[test]
fn a_note_at_the_queried_cursor_adds_one_user_line_and_keeps_the_cursor() {
    let f = Fixture::new();
    f.pending(4);
    f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0003","title":"Scribe screen","entries":[
            {"msg":"m-0003","quote":"ship parse and append first","tag":"decision","topics":[]}]}),
    );
    let lines = f.ok(
        "office.scribe-query",
        json!({"session_id":SESSION,"limit":1}),
    );
    let line = &lines["results"][0];
    assert_eq!(line["title"], "Scribe screen", "{lines}");
    assert_eq!(line["project"], "constellation");
    assert_eq!(line["scribed_through"], "m-0003");

    let note = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":line["scribed_through"],"entries":[
            {"text":"call the venue at 3pm","tag":"todo","by":"user"}]}),
    );
    assert_eq!(note["refused"], json!([]));
    assert_eq!(note["scribed_through"], "m-0003", "the cursor did not move");
    let added = note["added"].as_array().unwrap();
    assert_eq!(added.len(), 1);
    assert!(
        added[0]["text"]
            .as_str()
            .unwrap()
            .ends_with(" #todo \"call the venue at 3pm\" by: user"),
        "{note}"
    );
    let after = f.ok(
        "office.scribe-query",
        json!({"session_id":SESSION,"by":"user"}),
    );
    assert_eq!(after["results"][0]["msg"], "note");
    assert_eq!(after["results"][0]["quote"], "call the venue at 3pm");
}

/// A batch whose every entry is refused is a failed capture: it writes no SCR
/// file, makes no commit and leaves the cursor unset, so the messages stay
/// pending and a corrected entry captures them.
#[test]
fn an_all_refused_batch_writes_nothing_and_a_corrected_retry_captures_the_messages() {
    let f = Fixture::new();
    f.pending(4);
    let head = f.git(&["rev-parse", "HEAD"]);
    let rejected = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","entries":[
            {"msg":"m-0003","quote":"never spoken","tag":"decision","topics":[]}]}),
    );
    assert_eq!(rejected["refused"][0]["reason"], "quote_not_verbatim");
    assert_eq!(rejected["written"], false, "{rejected}");
    assert_eq!(rejected["scribed_through"], Value::Null, "{rejected}");
    let path = rejected["path"].as_str().unwrap();
    assert!(!f.library().join(path).exists(), "{rejected}");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["status", "--porcelain"]), "");

    let pending = f.pending(4);
    assert_eq!(pending["new_count"], 4, "{pending}");
    let retry = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","entries":[
            {"msg":"m-0003","quote":"ship parse and append first","tag":"decision","topics":[]}]}),
    );
    assert_eq!(retry["refused"], json!([]));
    assert_eq!(retry["written"], true, "{retry}");
    assert_eq!(retry["scribed_through"], "m-0004");
    let text = std::fs::read_to_string(f.library().join(path)).unwrap();
    assert!(text.contains("\"ship parse and append first\""), "{text}");
    assert!(!text.contains("never spoken"), "{text}");
}

/// The same rule for a file that already exists: a refused batch that also
/// carries a new title keeps the file's name, its cursor and the Library's
/// history, and the corrected retry captures the next message.
#[test]
fn an_all_refused_batch_into_an_existing_file_keeps_its_name_and_cursor() {
    let f = Fixture::new();
    f.pending(4);
    let first = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0002","entries":[
            {"msg":"m-0001","quote":"Plan the scribe v1 rollout.","tag":"decision","topics":[]}]}),
    );
    assert_eq!(first["scribed_through"], "m-0002");
    let path = first["path"].as_str().unwrap().to_owned();
    let before = std::fs::read_to_string(f.library().join(&path)).unwrap();
    let head = f.git(&["rev-parse", "HEAD"]);

    let rejected = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","title":"Scribe v1 rollout","entries":[
            {"msg":"m-0003","quote":"never spoken","tag":"decision","topics":[]}]}),
    );
    assert_eq!(rejected["refused"][0]["reason"], "quote_not_verbatim");
    assert_eq!(rejected["written"], false, "{rejected}");
    assert_eq!(rejected["renamed_from"], Value::Null, "{rejected}");
    assert_eq!(rejected["path"], path.as_str(), "{rejected}");
    assert_eq!(rejected["previous_through"], "m-0002");
    assert_eq!(rejected["scribed_through"], "m-0002", "{rejected}");
    assert_eq!(
        std::fs::read_to_string(f.library().join(&path)).unwrap(),
        before
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["status", "--porcelain"]), "");

    let retry = f.ok(
        "office.scribe-append",
        json!({"session_id":SESSION,"through":"m-0004","entries":[
            {"msg":"m-0003","quote":"ship parse and append first","tag":"decision","topics":[]}]}),
    );
    assert_eq!(retry["refused"], json!([]));
    assert_eq!(retry["path"], path.as_str());
    assert_eq!(retry["scribed_through"], "m-0004");
    let text = std::fs::read_to_string(f.library().join(&path)).unwrap();
    assert!(text.contains("\"ship parse and append first\""), "{text}");
}

fn tag_names(tags: &Value) -> Vec<&str> {
    tags.as_array()
        .unwrap()
        .iter()
        .map(|tag| tag["name"].as_str().unwrap())
        .collect()
}

fn config_text(f: &Fixture) -> String {
    std::fs::read_to_string(f.library().join(".office/config.yaml")).unwrap()
}

/// With no `scribe` key, status and scribe-pending report exactly the two
/// default tags and a 20 minute / 10 turn cadence.
#[test]
fn without_a_scribe_key_the_defaults_are_in_force() {
    let f = Fixture::new();
    assert!(!config_text(&f).contains("scribe"));
    let status = f.ok("office.status", json!({}));
    assert_eq!(
        status["scribe"]["cadence"],
        json!({"minutes": 20, "turns": 10})
    );
    assert_eq!(
        tag_names(&status["scribe"]["tags"]),
        ["preferences", "decisions"]
    );
    assert_eq!(status["scribe"]["tags_customized"], false);
    let tags = &status["scribe"]["tags"];
    assert_eq!(tags[0]["sink"], "preference_page");
    assert_eq!(tags[0]["mode"], "propose");
    assert_eq!(tags[1]["sink"], "decisions_ledger");
    assert_eq!(tags[1]["mode"], "append");
    assert_eq!(tags[0]["trigger"], "cadence");
    assert_eq!(tags[1]["trigger"], "cadence");
    let pending = f.pending(2);
    assert_eq!(pending["tags"], status["scribe"]["tags"]);
    assert_eq!(pending["cadence"], json!({"minutes": 20, "turns": 10}));
}

/// Valid `scribe.*` writes persist, show in status and scribe-pending, and
/// null restores the defaults.
#[test]
fn config_set_writes_cadence_and_tags_and_null_restores_defaults() {
    let f = Fixture::new();
    f.ok(
        "office.config-set",
        json!({"key":"scribe.cadence","value":{"minutes":45,"turns":6}}),
    );
    f.ok(
        "office.config-set",
        json!({"key":"scribe.tags.lesson","value":{
            "scope":"global","instruction":"A lesson learned.","sink":"orbit_task",
            "workspace":"polaris","capture":"markers+inferred","trigger":"marker"}}),
    );
    f.ok(
        "office.config-set",
        json!({"key":"scribe.tags.preferences","value":{
            "scope":"global","instruction":"Standing answers.","sink":"preference_page","mode":"apply"}}),
    );
    let status = f.ok("office.status", json!({}));
    assert_eq!(
        status["scribe"]["cadence"],
        json!({"minutes": 45, "turns": 6})
    );
    assert_eq!(status["scribe"]["tags_customized"], true);
    let tags = &status["scribe"]["tags"];
    assert_eq!(tag_names(tags), ["preferences", "decisions", "lesson"]);
    assert_eq!(tags[0]["mode"], "apply");
    assert_eq!(tags[2]["workspace"], "polaris");
    assert_eq!(tags[2]["capture"], "markers+inferred");
    assert_eq!(tags[2]["mode"], "propose");
    assert_eq!(tags[2]["trigger"], "marker");
    assert_eq!(tags[0]["trigger"], "cadence");
    let pending = f.pending(2);
    assert_eq!(pending["tags"], *tags);
    assert_eq!(pending["cadence"], json!({"minutes": 45, "turns": 6}));
    assert!(config_text(&f).contains("\"lesson\""));

    f.ok(
        "office.config-set",
        json!({"key":"scribe.tags.decisions","value":null}),
    );
    let status = f.ok("office.status", json!({}));
    assert_eq!(
        tag_names(&status["scribe"]["tags"]),
        ["preferences", "lesson"]
    );

    f.ok(
        "office.config-set",
        json!({"key":"scribe.tags","value":[{
        "name":"idea","scope":"session","instruction":"An idea.","sink":"scr_only"}]}),
    );
    let status = f.ok("office.status", json!({}));
    assert_eq!(tag_names(&status["scribe"]["tags"]), ["idea"]);

    f.ok(
        "office.config-set",
        json!({"key":"scribe.tags","value":null}),
    );
    f.ok(
        "office.config-set",
        json!({"key":"scribe.cadence","value":null}),
    );
    let status = f.ok("office.status", json!({}));
    assert_eq!(
        status["scribe"]["cadence"],
        json!({"minutes": 20, "turns": 10})
    );
    assert_eq!(
        tag_names(&status["scribe"]["tags"]),
        ["preferences", "decisions"]
    );
    assert!(!config_text(&f).contains("scribe"));
}

/// Each refused case carries a typed code and leaves the file untouched.
#[test]
fn config_set_refuses_invalid_scribe_values_with_typed_codes() {
    let f = Fixture::new();
    let before = config_text(&f);
    let tag = |sink: &str, extra: Value| {
        let mut definition = json!({"scope":"project","instruction":"Record.","sink":sink});
        definition
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        definition
    };
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "scribe.cadence",
            json!({"minutes": 4}),
            "invalid_library_setting",
        ),
        (
            "scribe.cadence",
            json!({"minutes": 241}),
            "invalid_library_setting",
        ),
        (
            "scribe.cadence",
            json!({"turns": 1}),
            "invalid_library_setting",
        ),
        (
            "scribe.cadence",
            json!({"turns": 101}),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("tracker", json!({})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("decisions_ledger", json!({"mode":"apply"})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("orbit_task", json!({"mode":"apply"})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("scr_only", json!({"workspace":"ws"})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("scr_only", json!({"trigger":"always"})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("scr_only", json!({"trigger":null})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.Bad_Name",
            tag("scr_only", json!({})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("scr_only", json!({"name":"other"})),
            "invalid_library_setting",
        ),
        (
            "scribe.tags",
            json!([{"name":"d","scope":"global","instruction":"i","sink":"scr_only"},
            {"name":"d","scope":"global","instruction":"i","sink":"scr_only"}]),
            "invalid_library_setting",
        ),
        (
            "scribe.tags.x",
            tag("/home/daniel/polaris/rules.md", json!({})),
            "tag_sink_refused",
        ),
        (
            "scribe.tags.x",
            tag("../almanac", json!({})),
            "tag_sink_refused",
        ),
        (
            "scribe.tags.x",
            tag("scr_only", json!({"path":"/tmp/out.md"})),
            "tag_sink_refused",
        ),
        ("scribe.mode", json!(1), "unknown_library_setting"),
    ];
    for (key, value, code) in cases {
        let response = f.call("office.config-set", json!({"key":key,"value":value}));
        assert_eq!(response["ok"], false, "{key} {value}");
        assert_eq!(response["error"]["code"], code, "{key} {value}: {response}");
        assert_eq!(config_text(&f), before, "{key} changed the config");
    }
}

/// No agent can change the block: `office.config-set` stays out of the MCP
/// surface (the manifest/spec agreement is `plugin_tree.rs`).
#[test]
fn config_set_is_not_agent_callable() {
    let spec = office_tools::tools()
        .iter()
        .find(|tool| tool.spec().name == "config-set")
        .unwrap()
        .spec();
    assert_eq!(spec.mcp_scope, office_core::tool::McpScope::None);
}
