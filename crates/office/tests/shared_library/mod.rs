//! Shared clone behavior through the shipped binary and real adapters.
use super::Env;
use serde_json::{Value, json};

fn put(env: &Env, path: &str, text: &str) {
    let path = env.state.join("shared/repo").join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn page(id: &str, title: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: {title}\nshared_by: alice\n---\n## Overview\nShared fixture knowledge.\n"
    )
}
fn output(env: &Env, tool: &str, input: Value) -> Value {
    let response = env.tool(tool, input);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

#[test]
fn shared_index_resolution_citations_and_own_precedence() {
    let env = Env::new();
    output(&env, "office.init", json!({}));
    output(
        &env,
        "office.config-set",
        json!({"key":"shared","value":{"remote":"/fixture/team.git","branch":"main","member":"me"}}),
    );
    put(
        &env,
        "knowledge/domain/dom-fixture.md",
        &page("dom-fixture", "Shared fixture"),
    );
    put(
        &env,
        "knowledge/domain/dom-private.md",
        &page("dom-private", "Private")
            .replace("shared_by: alice", "shared_by: alice\nsensitivity: private"),
    );
    put(
        &env,
        "knowledge/elsewhere.md",
        &page("dom-outside", "Outside"),
    );
    put(
        &env,
        "knowledge/domain/dom-malformed.md",
        "---\nid: dom-malformed\nstatus: bogus\nshared_by: alice\n---\nBad",
    );
    put(
        &env,
        "members/alice/sources/2026/10/src-20261028-fixture/source.md",
        "---\nid: src-20261028-fixture\nkind: meeting\ntitle: Shared transcript\nshared_by: alice\n---\n[00:00:10] Alice: First.\n[00:00:20] Alice: Last.\n",
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        env.state
            .join("shared/repo/knowledge/domain/dom-fixture.md"),
        env.state.join("shared/repo/knowledge/domain/dom-link.md"),
    )
    .unwrap();
    put(
        &env,
        "knowledge/domain/dom-template.md",
        &page("dom-template", "Invalid template").replace(
            "shared_by: alice",
            "shared_by: alice\ntemplate: missing-template",
        ),
    );
    put(
        &env,
        "knowledge/domain/dom-contact.md",
        &page("dom-contact", "Personal pointer").replace(
            "shared_by: alice",
            "shared_by: alice\nrefs: [{label: Owner, contact: owner@example.com}]",
        ),
    );
    #[cfg(unix)]
    {
        let fifo = env
            .state
            .join("shared/repo/knowledge/domain/dom-special.md");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(fifo)
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap())
                .env("HOME", env.workspace.join("home"))
                .status()
                .unwrap()
                .success()
        );
    }
    let indexed = output(&env, "office.reindex", json!({}));
    assert_eq!(indexed["indexed_documents"], 2, "{indexed}");
    put(
        &env,
        "members/alice/sources/2026/10/src-20261028-fixture/summary.md",
        "Sidecar searchable summary.",
    );
    output(&env, "office.reindex", json!({}));
    let sidecar = output(
        &env,
        "office.search",
        json!({"query":"searchable","layer":"shared"}),
    );
    assert_eq!(sidecar["hits"][0]["id"], "alice/src-20261028-fixture");
    let hits = output(
        &env,
        "office.search",
        json!({"query":"shared","layer":"shared"}),
    );
    assert_eq!(hits["count"], 2, "{hits}");
    for hit in hits["hits"].as_array().unwrap() {
        assert_eq!(hit["layer"], "shared");
        assert_eq!(hit["shared_by"], "alice");
    }
    for id in ["dom-fixture", "alice/dom-fixture"] {
        let shown = output(&env, "office.knowledge-show", json!({"id":id}));
        assert_eq!(shown["page"]["title"], "Shared fixture");
        assert_eq!(shown["page"]["layer"], "shared");
        assert_eq!(shown["page"]["shared_by"], "alice");
    }
    let shown = output(
        &env,
        "office.source-show",
        json!({"id":"alice/src-20261028-fixture"}),
    );
    assert_eq!(shown["layer"], "shared");
    assert_eq!(shown["shared_by"], "alice");
    let listed = output(&env, "office.knowledge-list", json!({}));
    assert_eq!(listed["pages"].as_array().unwrap().len(), 1);
    assert_eq!(listed["pages"][0]["layer"], "shared");
    assert_eq!(listed["pages"][0]["shared_by"], "alice");
    assert!(!listed.to_string().contains("owner@example.com"));
    let cite = |target: &str| {
        format!(
            "---\nid: dom-citing\ntitle: Citing\ncites: [\"{target}\"]\n---\n## Overview\nEvidence [{target}].\n"
        )
    };
    output(
        &env,
        "office.knowledge-write",
        json!({"markdown":cite("alice/src-20261028-fixture#t=00:00:15")}),
    );
    for target in [
        "alice/src-20261028-fixture#t=00:00:21",
        "nobody/src-20261028-fixture#t=00:00:15",
    ] {
        let refused = env.tool("office.knowledge-write", json!({"markdown":cite(target)}));
        assert_eq!(refused["error"]["code"], "unresolved_cite", "{refused}");
    }
    output(
        &env,
        "office.knowledge-write",
        json!({"markdown":page("dom-fixture", "Own fixture")}),
    );
    for id in ["dom-fixture", "me/dom-fixture"] {
        let shown = output(&env, "office.knowledge-show", json!({"id":id}));
        assert_eq!(shown["page"]["title"], "Own fixture");
        assert_eq!(shown["page"]["layer"], "own");
        assert_eq!(shown["page"]["shared_by"], Value::Null);
    }
    assert_eq!(
        output(
            &env,
            "office.knowledge-show",
            json!({"id":"alice/dom-fixture"})
        )["page"]["title"],
        "Shared fixture"
    );
}

#[test]
fn missing_shared_clone_is_empty() {
    let env = Env::new();
    output(&env, "office.init", json!({}));
    output(&env, "office.reindex", json!({}));
    assert_eq!(
        output(
            &env,
            "office.search",
            json!({"query":"anything","layer":"shared"})
        )["count"],
        0
    );
    assert!(!env.state.join("shared").exists());
}

// Offline callbacks with the real filesystem/index adapter, so proposals and
// attachments exercise their public dispatch paths without an Orbit service.
use office_core::ports::*;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Default)]
struct Callbacks {
    payload: Rc<RefCell<Option<Value>>>,
    artifacts: Rc<RefCell<std::collections::BTreeMap<String, String>>>,
}
impl Orbit for Callbacks {
    fn task_list(&self, _: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        Ok(json!({"tasks":[],"total":0,"truncated":false}))
    }
    fn task_add(&self, _: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        Ok(json!({"id":"fixture-proposal","status":"proposed"}))
    }
    fn task_show(&self, _: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        Ok(
            json!({"id":"fixture-proposal","status":"proposed","tags":["proposal","proposal:knowledge_update"],"title":"Fixture knowledge","description":"Fixture task","extra":null}),
        )
    }
    fn task_update(&self, input: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        Ok(Value::Object(input.clone()))
    }
    fn task_artifact_put(
        &self,
        input: &serde_json::Map<String, Value>,
    ) -> office_core::Result<Value> {
        let text = std::fs::read_to_string(input["source_path"].as_str().unwrap()).unwrap();
        self.artifacts
            .borrow_mut()
            .insert(input["path"].as_str().unwrap().into(), text);
        Ok(json!({}))
    }
    fn task_artifact_get(&self, _: &serde_json::Map<String, Value>) -> office_core::Result<Value> {
        let text = self.payload.borrow().as_ref().unwrap().to_string();
        Ok(json!({"content":text}))
    }
}
struct ModelsFixture;
impl Models for ModelsFixture {
    fn transcribe(&self, _: &TranscribeRequest) -> office_core::Result<Routed<Transcript>> {
        panic!("unexpected transcription")
    }
    fn extract(&self, request: &ExtractRequest) -> office_core::Result<Routed<Value>> {
        assert!(request.text.contains("Existing fixture knowledge"));
        assert!(!request.text.contains("owner@example.com"));
        Ok(Routed {
            value: json!({"body":"## Overview\nMerged fixture knowledge.","conflicts":[]}),
            derived_by: DerivedBy {
                route: office_core::domain::endpoint::ModelRoute::Hosted,
                model: "fixture".into(),
            },
        })
    }
    fn read_image(&self, _: &ImageRequest) -> office_core::Result<Routed<ImageReading>> {
        panic!("unexpected image read")
    }
    fn check_private_route(&self) -> office_core::Result<RouteCheck> {
        panic!("unexpected route check")
    }
}
struct NoScripts;
impl ScriptRunner for NoScripts {
    fn prepare(
        &self,
        _: &office_core::domain::script::ScriptPreparation,
    ) -> office_core::Result<()> {
        panic!("shared script must not prepare")
    }
    fn run(
        &self,
        _: &office_core::domain::script::ScriptExecution,
        _: &office_core::domain::script::ScriptBounds,
    ) -> office_core::Result<(
        office_core::domain::script::ScriptResult,
        Vec<office_core::domain::script::ScriptFile>,
    )> {
        panic!("shared script must not run")
    }
}
struct ClockFixture;
impl Clock for ClockFixture {
    fn now(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1793145600)
    }
}
fn ports(env: &Env, callbacks: Callbacks) -> Ports<'static> {
    Ports {
        library: Box::new(office_store::LibraryStore::open(&env.library(), &env.state).unwrap()),
        orbit: Box::new(callbacks),
        models: Box::new(ModelsFixture),
        scripts: Box::new(NoScripts),
        clock: Box::new(ClockFixture),
        calendar: Box::new(UnavailableCalendar),
        selections: Box::new(UnavailableSelections),
    }
}
fn dispatch(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let bytes = json!({"schema_version":1,"tool":tool,"input":input,"context":{}}).to_string();
    let request = office_core::protocol::parse(&bytes).unwrap();
    serde_json::from_str(&office_tools::serve(&request, ports)).unwrap()
}
fn dispatched(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let result = dispatch(ports, tool, input);
    assert_eq!(result["ok"], true, "{result}");
    result["output"].clone()
}

#[test]
fn attach_chooses_shared_then_own_and_records_each_layer_in_manifest() {
    let env = Env::new();
    output(&env, "office.init", json!({}));
    put(
        &env,
        "knowledge/domain/dom-fixture.md",
        &page("dom-fixture", "Fixture knowledge"),
    );
    output(&env, "office.reindex", json!({}));
    let callbacks = Callbacks::default();
    let ports = ports(&env, callbacks.clone());
    for layer in ["shared", "own"] {
        if layer == "own" {
            ports
                .library
                .write_document(
                    Path::new("knowledge/domain/dom-fixture.md"),
                    &page("dom-fixture", "Fixture own knowledge"),
                )
                .unwrap();
        }
        let result = dispatched(&ports, "office.attach", json!({"task":"fixture-proposal"}));
        assert_eq!(result["attached"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(result["attached"][0]["layer"], layer);
        let artifacts = callbacks.artifacts.borrow();
        let manifest: Value = serde_json::from_str(&artifacts["office-attach.json"]).unwrap();
        assert_eq!(manifest["attached"][0]["layer"], layer);
        assert_eq!(manifest["attached"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn bootstrap_updates_shared_page_and_acceptance_creates_shareable_own_copy() {
    use office_core::domain::source::sha256_hex;
    let env = Env::new();
    output(&env, "office.init", json!({}));
    let shared = "---\nid: dom-topic-fixture\ntemplate: topic\ntitle: Fixture knowledge\nshared_by: alice\n---\n## Overview\nExisting fixture knowledge.\n";
    put(&env, "knowledge/domain/dom-topic-fixture.md", shared);
    output(&env, "office.reindex", json!({}));
    let callbacks = Callbacks::default();
    let ports = ports(&env, callbacks.clone());
    let path = "sources/2026/10/src-20261028-design/source.md";
    ports.library.write_document(Path::new(path), "---\nid: src-20261028-design\nkind: doc\ntitle: Design\ntags: [bootstrap:docs]\n---\n# Design\nFixture knowledge.\n").unwrap();
    let preview = dispatched(&ports, "office.batch-plan", json!({"phase":"docs"}));
    dispatched(
        &ports,
        "office.batch-plan",
        json!({"phase":"docs","confirm":true,"digest":preview["plan"]["digest"]}),
    );
    let digest = json!({"summary":"Fixture digest","kind_hint":"design-doc","as_of":"2026-10-28","subjects":[{"name":"Fixture knowledge","template":"topic"}],"claims":[],"procedures":[],"terms":[],"refs":[]});
    std::fs::write(
        env.library()
            .join(Path::new(path).with_file_name("digest.json")),
        digest.to_string(),
    )
    .unwrap();
    let status = dispatched(&ports, "office.bootstrap-status", json!({}));
    assert_eq!(status["phases"][0]["shared"], 1, "{status}");
    let result = dispatched(&ports, "office.consolidate", json!({"batch_id":"docs"}));
    assert_eq!(result["failed"], json!([]), "{result}");
    assert_eq!(result["merged"][0]["page"], "dom-topic-fixture");
    let payload: Value =
        serde_json::from_str(&callbacks.artifacts.borrow()["proposal.json"]).unwrap();
    assert_eq!(payload["kind"], "knowledge_update");
    assert_eq!(payload["base_sha256"], sha256_hex(shared.as_bytes()));
    let shared_path = Path::new("shared/knowledge/domain/dom-topic-fixture.md");
    let wrong = ports
        .library
        .write_reviewed_page(
            shared_path,
            &payload["markdown"]
                .as_str()
                .unwrap()
                .replace("dom-topic-fixture", "dom-topic-other"),
            Some(&sha256_hex(shared.as_bytes())),
        )
        .unwrap_err();
    assert_eq!(wrong.code(), "proposal_revision_changed");
    let stale = ports
        .library
        .write_reviewed_page(
            shared_path,
            payload["markdown"].as_str().unwrap(),
            Some(&"0".repeat(64)),
        )
        .unwrap_err();
    assert_eq!(stale.code(), "proposal_revision_changed");
    assert!(
        !env.library()
            .join("knowledge/domain/dom-topic-fixture.md")
            .exists()
    );
    *callbacks.payload.borrow_mut() = Some(payload.clone());
    dispatched(
        &ports,
        "office.proposal-accept",
        json!({"id":"fixture-proposal","sha256":sha256_hex(payload.to_string().as_bytes())}),
    );
    let own = ports.library.get("dom-topic-fixture").unwrap().unwrap();
    assert_eq!(own.layer, office_core::domain::knowledge::LibraryLayer::Own);
    assert!(own.shareable);
    let raced = ports
        .library
        .write_reviewed_page(
            shared_path,
            payload["markdown"].as_str().unwrap(),
            Some(&sha256_hex(shared.as_bytes())),
        )
        .unwrap_err();
    assert_eq!(raced.code(), "proposal_revision_changed");

    assert_eq!(
        std::fs::read_to_string(
            env.state
                .join("shared/repo/knowledge/domain/dom-topic-fixture.md")
        )
        .unwrap(),
        shared
    );
    assert_eq!(ports.library.bootstrap_snapshot().unwrap().pages.len(), 1);
}

#[test]
fn shared_artifacts_show_qualified_metadata_and_scripts_never_execute() {
    let env = Env::new();
    output(&env, "office.init", json!({}));
    for (kind, file) in [("report", "nested/report.md"), ("script", "script.py")] {
        let id = format!("art-20261028-{kind}");
        let text = format!(
            "---\nid: {id}\nkind: {kind}\ntitle: Shared {kind}\nsummary: Fixture\nfiles: [{file}]\ntask: fixture-task\nproduced_by: {{crew: null, runbook: null, at: null}}\napproved: {{by: user, at: \"2026-10-28T00:00:00Z\", edited: false}}\ncites: []\nseries: fixture\nversion: 1\ntags: []\nshared_by: alice\n---\n"
        );
        put(
            &env,
            &format!("members/alice/artifacts/2026/10/{id}/artifact.md"),
            &text,
        );
        put(
            &env,
            &format!("members/alice/artifacts/2026/10/{id}/{file}"),
            "Fixture body.\n",
        );
    }
    output(&env, "office.reindex", json!({}));
    let ports = ports(&env, Callbacks::default());
    let shown = dispatched(
        &ports,
        "office.artifact-show",
        json!({"id":"alice/art-20261028-report"}),
    );
    assert_eq!(shown["layer"], "shared");
    assert_eq!(shown["shared_by"], "alice");
    assert_eq!(shown["artifact"]["id"], "alice/art-20261028-report");
    assert_eq!(shown["body"], "Fixture body.\n");
    for id in ["art-20261028-script", "alice/art-20261028-script"] {
        let refused = dispatch(
            &ports,
            "office.script-run",
            json!({"artifact":id,"sha256":"0".repeat(64),"inputs":[],"args":[],"output":{"title":"Result","kind":"data","series":"result"}}),
        );
        assert_eq!(refused["ok"], false, "{refused}");
    }
}

#[test]
fn equal_evidence_ids_are_member_scoped_and_own_member_aliases_stay_local() {
    let env = Env::new();
    output(&env, "office.init", json!({}));
    output(
        &env,
        "office.config-set",
        json!({"key":"shared","value":{"remote":"/fixture/team.git","branch":"main","member":"me"}}),
    );
    let id = "src-20261028-same";
    for member in ["alice", "bob"] {
        put(
            &env,
            &format!("members/{member}/sources/2026/10/{id}/source.md"),
            &format!(
                "---\nid: {id}\nkind: note\ntitle: Same evidence\nshared_by: {member}\n---\nFrom {member}.\n"
            ),
        );
    }
    output(&env, "office.reindex", json!({}));
    let store = office_store::LibraryStore::open(&env.library(), &env.state).unwrap();
    store
        .write_document(
            Path::new(&format!("sources/2026/10/{id}/source.md")),
            &format!("---\nid: {id}\nkind: note\ntitle: Same evidence\n---\nOwn evidence.\n"),
        )
        .unwrap();
    for member in ["alice", "bob"] {
        let shown = output(
            &env,
            "office.source-show",
            json!({"id":format!("{member}/{id}")}),
        );
        assert_eq!(shown["body"], format!("From {member}."));
        assert_eq!(shown["shared_by"], member);
    }
    let shown = output(&env, "office.source-show", json!({"id":format!("me/{id}")}));
    assert_eq!(shown["body"], "Own evidence.");
    assert_eq!(shown["layer"], "own");
    assert_eq!(shown["shared_by"], Value::Null);
    let hits = output(
        &env,
        "office.search",
        json!({"query":"evidence","layer":"shared","limit":1}),
    );
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1);
    assert_eq!(hits["hits"][0]["layer"], "shared");
    let hits = output(
        &env,
        "office.search",
        json!({"query":"evidence","layer":"own","limit":1}),
    );
    assert_eq!(hits["hits"][0]["layer"], "own");
    // An own-member qualified self-citation cannot evade the existing guard.
    output(
        &env,
        "office.knowledge-write",
        json!({"markdown":"---\nid: dom-self\ntitle: Self\n---\nBody.\n"}),
    );
    let refused = env.tool(
        "office.knowledge-write",
        json!({"markdown":"---\nid: dom-self\ntitle: Self\ncites: [me/dom-self]\n---\nBody.\n"}),
    );
    assert_eq!(refused["error"]["code"], "unresolved_cite");
}
