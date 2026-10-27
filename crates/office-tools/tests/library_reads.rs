//! Library screen reads through the public dispatch entry point, without host I/O.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use office_core::domain::knowledge::{Backlink, Revision};
use office_core::ports::{CaptureSourceRequest, Library, LibraryText, Ports, SourceView};
use office_testkit::{envelope, fakes::FakeLibrary};
use serde_json::{Value, json};

fn ports(library: impl Library + 'static) -> Ports<'static> {
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports
}

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn page(library: &FakeLibrary, id: &str, fields: &str) {
    library
        .write_document(
            Path::new(&format!("knowledge/domain/{id}.md")),
            &format!("---\nid: {id}\ntitle: {id}\n{fields}---\nPage body.\n"),
        )
        .unwrap();
}

#[test]
fn lists_all_pages_and_missing_singleton_templates_before_filtering() {
    let library = FakeLibrary::with_bundled_templates();
    page(&library, "dom-user", "template: user\nsummary: About me\n");
    page(
        &library,
        "dom-person-lee",
        "template: person\nstatus: retired\nsensitivity: private\n",
    );
    page(&library, "dom-notes", "");
    let ports = ports(library);
    let listed = call(&ports, "office.knowledge-list", json!({}));
    assert_eq!(listed["ok"], true, "{listed}");
    let output = &listed["output"];
    let ids: Vec<_> = output["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["dom-notes", "dom-person-lee", "dom-user"]);
    assert_eq!(output["pages"][0]["template"], Value::Null);
    assert_eq!(output["pages"][1]["status"], "retired");
    assert_eq!(output["pages"][1]["sensitivity"], "private");
    assert_eq!(output["pages"][2]["summary"], "About me");
    let missing: Vec<_> = output["missing_templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["template"].as_str().unwrap())
        .collect();
    assert_eq!(
        missing,
        [
            "glossary",
            "goals",
            "rhythm",
            "standing-directions",
            "systems"
        ]
    );

    let filtered = call(
        &ports,
        "office.knowledge-list",
        json!({"template":"person"}),
    );
    assert_eq!(filtered["ok"], true, "{filtered}");
    assert_eq!(filtered["output"]["pages"], json!([output["pages"][1]]));
    assert_eq!(filtered["output"]["missing_templates"], json!([]));
    let existing = call(&ports, "office.knowledge-list", json!({"template":"user"}));
    assert_eq!(existing["output"]["missing_templates"], json!([]));
    let missing = call(
        &ports,
        "office.knowledge-list",
        json!({"template":"systems"}),
    );
    assert_eq!(missing["output"]["pages"], json!([]));
    assert_eq!(
        missing["output"]["missing_templates"][0]["id"],
        "dom-systems"
    );
    let unknown = call(
        &ports,
        "office.knowledge-list",
        json!({"template":"unknown"}),
    );
    assert_eq!(
        unknown["output"],
        json!({"pages":[], "missing_templates":[]})
    );
}

#[test]
fn list_refuses_invalid_filters_unknown_keys_and_uninitialized_libraries() {
    let ports = ports(FakeLibrary::with_bundled_templates());
    for input in [
        json!({"template":null}),
        json!({"template":42}),
        json!({"template":""}),
        json!({"template":" person "}),
        json!({"template":"Person"}),
        json!({"extra":true}),
    ] {
        let response = call(&ports, "office.knowledge-list", input);
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
    let empty = call(
        &ports,
        "office.knowledge-list",
        json!({"template":"person"}),
    );
    assert_eq!(empty["output"], json!({"pages":[], "missing_templates":[]}));
    let unavailable = call(&uninitialized_ports(), "office.knowledge-list", json!({}));
    assert_eq!(unavailable["ok"], false, "{unavailable}");
}

fn uninitialized_ports() -> Ports<'static> {
    ports(FakeLibrary::default())
}

#[test]
fn list_refuses_an_incomplete_corpus_instead_of_reporting_false_absence() {
    let library = FakeLibrary::with_bundled_templates();
    page(&library, "dom-user", "template: user\nstatus: invalid\n");
    let response = call(&ports(library), "office.knowledge-list", json!({}));
    assert_eq!(
        response["error"]["code"], "invalid_frontmatter",
        "{response}"
    );
    let mut library = FakeLibrary::with_bundled_templates();
    library.templates[0].markdown = None;
    let response = call(&ports(library), "office.knowledge-list", json!({}));
    assert_eq!(
        response["error"]["code"], "library_document_unreadable",
        "{response}"
    );
}

fn fixture(count: usize) -> (FakeLibrary, Vec<(&'static str, String, String)>) {
    let library = FakeLibrary::with_bundled_templates();
    page(&library, "dom-notes", "");
    let artifact_path = "artifacts/1970/01/art-19700101-data/artifact.md";
    library.write_document(Path::new(artifact_path), "---\nid: art-19700101-data\nkind: data\ntitle: Data\nsummary: Fixture\nfiles: [data.csv]\ntask: fixture-task\nproduced_by: { crew: fixture-crew, at: \"1970-01-01T00:00:00Z\" }\napproved: { by: user, at: \"1970-01-01T00:00:00Z\", edited: false }\ncites: []\nseries: data\nversion: 1\nsensitivity: normal\ntags: []\n---\n").unwrap();
    let source = library
        .capture_source(&CaptureSourceRequest {
            kind: Some("chat".into()),
            title: Some("Thread".into()),
            text: Some("one\ntwo".into()),
            captured_at: "1970-01-01T00:00:00Z".into(),
            ..CaptureSourceRequest::default()
        })
        .unwrap();
    let documents = vec![
        (
            "office.knowledge-show",
            "dom-notes".into(),
            "knowledge/domain/dom-notes.md".into(),
        ),
        (
            "office.artifact-show",
            "art-19700101-data".into(),
            artifact_path.into(),
        ),
        ("office.source-show", source.id, source.path),
    ];
    for (_, _, path) in &documents {
        library
            .set_history(
                Path::new(path),
                (0..count)
                    .map(|i| Revision {
                        commit: format!("{i:040x}"),
                        author: "Author".into(),
                        email: "author@example.com".into(),
                        authored_at: "1970-01-01T00:00:00Z".into(),
                        message: format!("Revision {i} of {path}"),
                    })
                    .collect(),
            )
            .unwrap();
    }
    (library, documents)
}

#[test]
fn every_show_returns_bounded_history_and_preserves_omitted_and_false_responses() {
    for count in [0, 1, 50, 51] {
        let (library, documents) = fixture(count);
        let ports = ports(library);
        let head = ports.library.head_commit().unwrap();
        for (tool, id, path) in documents {
            let plain = call(&ports, tool, json!({"id":id}));
            assert_eq!(plain["ok"], true, "{plain}");
            assert!(plain["output"].get("history").is_none());
            assert_eq!(plain, call(&ports, tool, json!({"id":id, "history":false})));
            let mut shown = call(&ports, tool, json!({"id":id, "history":true}));
            assert_eq!(shown["ok"], true, "{shown}");
            let history = shown["output"]
                .as_object_mut()
                .unwrap()
                .remove("history")
                .unwrap();
            assert_eq!(shown, plain, "history must only add the requested field");
            assert_eq!(history["total"], count);
            assert_eq!(history["truncated"], count > 50);
            assert_eq!(
                history["revisions"].as_array().unwrap().len(),
                count.min(50)
            );
            if count > 0 {
                assert_eq!(
                    history["revisions"][0],
                    json!({"commit":format!("{:040x}",0),"time":"1970-01-01T00:00:00Z","summary":format!("Revision 0 of {path}")})
                );
                assert_eq!(
                    history["revisions"][count.min(50) - 1]["summary"],
                    format!("Revision {} of {path}", count.min(50) - 1)
                );
            }
        }
        assert_eq!(
            ports.library.head_commit().unwrap(),
            head,
            "show must not write"
        );
    }
}

// Default Library::history refuses reads. Delegating only the document reads
// proves that omitted/false history never requests the unavailable capability.
struct WithoutHistory(FakeLibrary);

impl Library for WithoutHistory {
    fn is_initialized(&self) -> office_core::Result<bool> {
        self.0.is_initialized()
    }
    fn knowledge_pages(&self) -> office_core::Result<Vec<LibraryText>> {
        self.0.knowledge_pages()
    }
    fn artifact_documents(&self) -> office_core::Result<Vec<LibraryText>> {
        self.0.artifact_documents()
    }
    fn backlinks(&self, id: &str) -> office_core::Result<Vec<Backlink>> {
        self.0.backlinks(id)
    }
    fn read_source(&self, id: &str) -> office_core::Result<SourceView> {
        self.0.read_source(id)
    }
}

#[test]
fn unavailable_history_is_only_an_error_when_requested() {
    let (library, documents) = fixture(1);
    let ports = ports(WithoutHistory(library));
    for (tool, id, _) in documents {
        for input in [json!({"id":id}), json!({"id":id,"history":false})] {
            let response = call(&ports, tool, input);
            assert_eq!(response["ok"], true, "{response}");
        }
        let response = call(&ports, tool, json!({"id":id,"history":true}));
        assert_eq!(
            response["error"]["code"], "library_unavailable",
            "{response}"
        );
    }
}

#[test]
fn source_history_uses_the_resolved_source_path_even_with_an_alias_and_anchor() {
    let (library, documents) = fixture(1);
    let id = &documents[2].1;
    let response = call(
        &ports(library),
        "office.source-show",
        json!({"source_id":id,"anchor":"#L2","history":true}),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["body"], "two");
    assert_eq!(response["output"]["history"]["total"], 1);
}

#[test]
fn every_show_refuses_non_boolean_history_and_unknown_input() {
    let (library, documents) = fixture(1);
    let ports = ports(library);
    for (tool, id, _) in documents {
        for value in [Value::Null, json!("true"), json!(1), json!([])] {
            let response = call(&ports, tool, json!({"id":id, "history":value}));
            assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
        }
        let response = call(
            &ports,
            tool,
            json!({"id":id, "history":true, "extra":false}),
        );
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
}

#[test]
fn knowledge_list_conformance_golden_replays_through_public_dispatch() {
    let ports = ports(FakeLibrary::with_bundled_templates());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/knowledge-list.yaml");
    let golden = std::fs::read_to_string(path).unwrap();
    let mut input = None;
    let mut cases = 0;
    for line in golden.lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let response = call(&ports, "office.knowledge-list", input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 5, "all success and refusal golden cases must run");
}

#[test]
fn knowledge_list_nonempty_conformance_case_replays_through_public_dispatch() {
    let library = FakeLibrary::with_bundled_templates();
    library.write_document(Path::new("knowledge/domain/dom-user.md"), "---\nid: dom-user\ntemplate: user\ntitle: Dee\n---\n## Name\nDee\n\n## Role\nOperations lead.\n").unwrap();
    let ports = ports(library);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/knowledge-show.yaml");
    let golden = std::fs::read_to_string(path).unwrap();
    let mut is_list = false;
    let mut input = None;
    let mut cases = 0;
    for line in golden.lines() {
        if let Some(tool) = line.strip_prefix("    tool: ") {
            is_list = tool == "knowledge-list";
        }
        if !is_list {
            continue;
        }
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        if let Some(value) = line.strip_prefix("      output: ") {
            let expected: Value = serde_json::from_str(value).unwrap();
            let response = call(&ports, "office.knowledge-list", input.take().unwrap());
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["output"], expected);
            cases += 1;
        }
    }
    assert_eq!(cases, 1, "the nonempty fixture golden must run");
}
