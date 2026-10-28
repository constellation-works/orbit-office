//! Sharing through public dispatch, with isolated Library state.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::ports::Ports;
use office_testkit::{envelope, fakes::PortsBuilder};
use serde_json::{Value, json};
use std::path::Path;

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

#[test]
fn share_set_preserves_documents_and_refuses_unshared_citations() {
    let ports = PortsBuilder::default().build();
    let source = "sources/2026/10/src-evidence/source.md";
    let text = "---\nid: src-evidence\nkind: note\ntitle: Evidence\nsensitivity: normal\n---\nExact body.\n";
    ports
        .library
        .write_document(Path::new(source), text)
        .unwrap();
    let result = call(
        &ports,
        "office.share-set",
        json!({"id":"src-evidence","shareable":true}),
    );
    assert_eq!(
        result["output"],
        json!({"id":"src-evidence","path":source,"shareable":true}),
        "{result}"
    );
    assert_eq!(
        ports.library.read_text(Path::new(source)).unwrap(),
        text.replacen("---\nExact", "shareable: true\n---\nExact", 1)
    );
    let page =
        "---\nid: dom-page\ntitle: Page\ncites: [src-evidence]\n---\nEvidence [src-evidence].\n";
    ports
        .library
        .write_document(Path::new("knowledge/domain/dom-page.md"), page)
        .unwrap();
    assert_eq!(
        call(
            &ports,
            "office.share-set",
            json!({"id":"src-evidence","shareable":false})
        )["ok"],
        true
    );
    let refused = call(
        &ports,
        "office.share-set",
        json!({"id":"dom-page","shareable":true}),
    );
    assert_eq!(
        refused["error"]["code"], "share_cites_unshared",
        "{refused}"
    );
    assert_eq!(
        ports
            .library
            .read_text(Path::new("knowledge/domain/dom-page.md"))
            .unwrap(),
        page
    );
}

#[test]
fn source_privacy_response_reports_atomic_unsharing_and_unknown_inputs_write_nothing() {
    let ports = PortsBuilder::default().build();
    let path = Path::new("sources/2026/10/src-private/source.md");
    ports
        .library
        .write_document(
            path,
            "---\nid: src-private\nkind: note\ntitle: Private\nshareable: true\n---\nBody.\n",
        )
        .unwrap();
    let response = call(
        &ports,
        "office.source-set-sensitivity",
        json!({"id":"src-private","sensitivity":"private"}),
    );
    assert_eq!(response["output"]["unshared"], true, "{response}");
    assert!(!ports.library.get("src-private").unwrap().unwrap().shareable);
    let head = ports.library.head_commit().unwrap();
    for (input, code) in [
        (
            json!({"id":"src-private","shareable":true}),
            "share_private_refused",
        ),
        (
            json!({"id":"src-private","shareable":true,"unknown":true}),
            "invalid_argument",
        ),
        (
            json!({"id":"src-private","shareable":"true"}),
            "invalid_argument",
        ),
        (
            json!({"id":"dom-missing","shareable":false}),
            "document_not_found",
        ),
    ] {
        assert_eq!(
            call(&ports, "office.share-set", input)["error"]["code"],
            code
        );
        assert_eq!(ports.library.head_commit().unwrap(), head);
    }
    let spec = office_tools::tools()
        .iter()
        .find(|tool| tool.spec().name == "share-set")
        .unwrap()
        .spec();
    assert_eq!(spec.mcp_scope, office_core::tool::McpScope::None);
    assert_eq!(
        spec.execution_kind,
        office_core::tool::ExecutionKind::Mutating
    );
}

#[test]
fn sharing_conformance_cases_replay_through_dispatch() {
    let ports = PortsBuilder::default().build();
    for (id, extra) in [("dom-user", ""), ("dom-client-acme", "cites: [dom-user]\n")] {
        ports
            .library
            .write_document(
                Path::new(&format!("knowledge/domain/{id}.md")),
                &format!("---\nid: {id}\ntitle: Fixture\n{extra}---\nBody.\n"),
            )
            .unwrap();
    }
    let golden = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugin/.orbit-plugin/tests/conformance/share-set.yaml"),
    )
    .unwrap();
    let mut input = None;
    let mut count = 0;
    for line in golden.lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let response = call(&ports, "office.share-set", input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 7);
}
