//! The digest since a meeting through the public exec API, with offline recording ports.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use office_core::ports::{Library, Ports};
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeOrbit},
};
use serde_json::{Value, json};

const SOURCE: &str = "src-20261102-sync";

fn call(ports: &Ports<'_>) -> Value {
    let request = office_core::protocol::parse(&envelope::request(
        "office.source-digest",
        json!({"source": SOURCE}),
        json!({}),
    ))
    .unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn task(id: &str, title: &str, tags: &[&str]) -> Value {
    let mut tags: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
    tags.push(format!("source:{SOURCE}"));
    json!({"id":id,"title":title,"status":"backlog","tags":tags,"description":""})
}

/// A followed workspace that resolves to the office's own task list (the
/// office workspace listed again in `brief.task_workspaces`) returns the
/// same tasks; each need is listed once.
#[test]
fn source_digest_lists_each_need_once_when_workspaces_return_the_same_tasks() {
    let mut library = FakeLibrary::initialized();
    library.settings = library
        .settings
        .with_setting("brief.task_workspaces", json!(["ws_office"]))
        .unwrap();
    library
        .write_document(
            Path::new(&format!("sources/2026/11/{SOURCE}/source.md")),
            &format!(
                "---\nid: {SOURCE}\nkind: meeting\ntitle: Eng sync\n---\n[00:00:01] A: Hello.\n"
            ),
        )
        .unwrap();
    library
        .insert_source_sidecar(
            SOURCE,
            "extract-result.json",
            &json!({"decisions":[{"text":"Rotate keys monthly.","cites":["#L1"]}]}).to_string(),
        )
        .unwrap();
    let tasks = json!([
        task(
            "T-1",
            "Contradiction: key rotation",
            &["owner:you", "office-scribe-contradictions"]
        ),
        task("T-2", "Book the pilot review", &["owner:you"]),
        task("T-3", "Tell the access desk", &["owner:you"]),
    ]);
    let listing = json!({"tasks": tasks, "total": 3, "truncated": false});
    let decisions =
        json!({"requests":[{"id":"DR-1","task_id":"T-2","state":"open","question":"Which week?"}]});
    let orbit = FakeOrbit::default();
    for reply in [&listing, &listing, &decisions, &decisions] {
        orbit.push_response(Ok(reply.clone()));
    }
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports.orbit = Box::new(orbit);

    let response = call(&ports);
    assert_eq!(response["ok"], true, "{response}");
    let needs = response["output"]["needs_you"].as_array().unwrap();
    let keys: Vec<(&str, &str)> = needs
        .iter()
        .map(|n| (n["kind"].as_str().unwrap(), n["task"].as_str().unwrap()))
        .collect();
    assert_eq!(
        keys,
        [
            ("contradiction", "T-1"),
            ("to_do", "T-2"),
            ("to_do", "T-3"),
            ("decision", "T-2"),
        ],
        "{response}"
    );
    // The office workspace's copy is kept.
    assert!(needs.iter().all(|n| n["workspace"].is_null()), "{response}");
}
