//! `office.library-sync` through public dispatch with the shared repository
//! fake: push, no-op, incoming updates, rejection, failures and dry runs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::ports::Ports;
use office_testkit::envelope;
use office_testkit::fakes::{FakeOrbit, FakeShared, PortsBuilder};
use serde_json::{Value, json};
use std::path::Path;

const PAGE: &str = "knowledge/domain/dom-topic.md";
const SOURCE: &str = "sources/2026/10/src-20261028-call/source.md";

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn sync(ports: &Ports<'_>, input: Value) -> Value {
    let response = call(ports, "office.library-sync", input);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

fn page(title: &str, body: &str, shareable: bool) -> String {
    format!(
        "---\nid: dom-topic\ntitle: {title}\nshareable: {shareable}\n---\n## Overview\n{body}\n"
    )
}

/// Configured ports with one shareable page, one shareable source with a
/// sidecar and one unshared page.
fn configured() -> (Ports<'static>, FakeShared, FakeOrbit) {
    let mut ports = PortsBuilder::default().build();
    let shared = FakeShared::default();
    let orbit = FakeOrbit::default();
    ports.shared = Box::new(shared.clone());
    ports.orbit = Box::new(orbit.clone());
    ports
        .library
        .set_setting(
            "shared",
            json!({"remote":"https://git.example.invalid/team.git","branch":"main","member":"me"}),
        )
        .unwrap();
    let library = &ports.library;
    library
        .write_document(Path::new(PAGE), &page("Topic", "Shared knowledge.", true))
        .unwrap();
    library
        .write_document(
            Path::new("knowledge/domain/dom-private-notes.md"),
            "---\nid: dom-private-notes\ntitle: Notes\n---\n## Overview\nMine.\n",
        )
        .unwrap();
    library
        .write_document(
            Path::new(SOURCE),
            "---\nid: src-20261028-call\nkind: meeting\ntitle: Call\nshareable: true\n---\n[00:00:01] A: Hello.\n",
        )
        .unwrap();
    (ports, shared, orbit)
}

#[test]
fn input_and_configuration_are_checked_before_any_git() {
    let mut ports = PortsBuilder::default().build();
    let shared = FakeShared::default();
    ports.shared = Box::new(shared.clone());
    for (input, code) in [
        (json!({"direction":"sideways"}), "invalid_argument"),
        (json!({"dry_run":"yes"}), "invalid_argument"),
        (json!({"token":"x"}), "invalid_argument"),
        (json!({}), "shared_not_configured"),
    ] {
        let response = call(&ports, "office.library-sync", input.clone());
        assert_eq!(response["error"]["code"], code, "{input} {response}");
    }
    assert_eq!(shared.fetches(), 0);
}

#[test]
fn first_push_initializes_the_layout_and_a_second_sync_pushes_nothing() {
    let (ports, shared, _) = configured();
    let first = sync(&ports, json!({}));
    let dir = "members/me/sources/2026/10/src-20261028-call";
    assert_eq!(
        first["pushed"],
        json!([PAGE, format!("{dir}/source.md")]),
        "{first}"
    );
    assert_eq!(first["skipped"], json!([]));
    let files = shared.remote_files();
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        [".office-shared.yaml", PAGE, &format!("{dir}/source.md")]
    );
    let text = String::from_utf8(files[PAGE].clone()).unwrap();
    assert!(
        text.contains("shared_by: \"me\"") && !text.contains("shareable"),
        "{text}"
    );
    assert_eq!(shared.commits(), 1);
    assert_eq!(first["commit"], json!(format!("{:040x}", 1)));
    assert!(first["synced_at"].is_string());
    let second = sync(&ports, json!({}));
    assert_eq!(second["pushed"], json!([]), "{second}");
    assert_eq!(second["pulled"], json!([]));
    assert_eq!(shared.commits(), 1);
    assert_eq!(shared.pushes().len(), 1);
}

#[test]
fn colleague_change_files_one_incoming_update_and_holds_the_page() {
    let (ports, shared, orbit) = configured();
    sync(&ports, json!({}));
    let theirs = String::from_utf8(shared.remote_files()[PAGE].clone())
        .unwrap()
        .replace("shared_by: \"me\"", "shared_by: \"bob\"")
        .replace("Shared knowledge.", "Better shared knowledge.");
    shared.put_remote(PAGE, theirs.as_bytes());
    let none = json!({"tasks":[],"total":0,"truncated":false});
    orbit.push_response(Ok(none.clone()));
    orbit.push_response(Ok(none.clone()));
    orbit.push_response(Ok(json!({"id":"proposal-1","status":"proposed"})));
    orbit.push_response(Ok(json!({})));
    let result = sync(&ports, json!({}));
    assert_eq!(result["pulled"], json!([PAGE]), "{result}");
    assert_eq!(
        result["incoming"],
        json!([{"page":"dom-topic","proposal":"proposal-1","created":true}])
    );
    assert_eq!(result["conflicts"], json!([]));
    assert_eq!(result["pushed"], json!([]));
    assert_eq!(shared.pushes().len(), 1);
    let added: Vec<_> = orbit
        .calls()
        .into_iter()
        .filter(|call| call.tool == "orbit.task.add")
        .collect();
    assert_eq!(added.len(), 1);
    let tags = added[0].input["tags"].to_string();
    assert!(tags.contains("proposal:knowledge_update") && tags.contains("shared-library"));
    // The same shared blob again: the open proposal is reported, not refiled.
    orbit.push_response(Ok(json!({"tasks":[{"id":"proposal-1","status":"proposed","tags":added[0].input["tags"]}],"total":1,"truncated":false})));
    let again = sync(&ports, json!({}));
    assert_eq!(
        again["incoming"],
        json!([{"page":"dom-topic","proposal":"proposal-1","created":false}])
    );
    assert_eq!(
        orbit
            .calls()
            .iter()
            .filter(|call| call.tool == "orbit.task.add")
            .count(),
        1
    );
    // Changing the own copy as well turns it into a conflict.
    ports
        .library
        .write_document(Path::new(PAGE), &page("Topic", "My own edit.", true))
        .unwrap();
    orbit.push_response(Ok(none.clone()));
    orbit.push_response(Ok(none));
    orbit.push_response(Ok(json!({"id":"proposal-2","status":"proposed"})));
    orbit.push_response(Ok(json!({})));
    let conflict = sync(&ports, json!({}));
    assert_eq!(
        conflict["conflicts"],
        json!([{"page":"dom-topic","proposal":"proposal-2","created":true}]),
        "{conflict}"
    );
    assert_eq!(conflict["pushed"], json!([]));
    assert_eq!(shared.pushes().len(), 1);
}

#[test]
fn unsharing_removes_this_members_paths_on_the_next_push() {
    let (ports, shared, _) = configured();
    sync(&ports, json!({}));
    let text = ports.library.read_text(Path::new(SOURCE)).unwrap();
    ports
        .library
        .write_document(
            Path::new(SOURCE),
            &text.replace("shareable: true", "shareable: false"),
        )
        .unwrap();
    let result = sync(&ports, json!({"direction":"push"}));
    assert_eq!(
        result["removed"],
        json!(["members/me/sources/2026/10/src-20261028-call/source.md"]),
        "{result}"
    );
    assert_eq!(
        shared.remote_files().keys().collect::<Vec<_>>(),
        [".office-shared.yaml", PAGE]
    );
    assert_eq!(
        shared.pushes()[1].message,
        "me: share 0 documents, remove 1"
    );
}

#[test]
fn a_rejected_push_is_retried_once_then_refused() {
    let (ports, shared, _) = configured();
    shared.reject_pushes(1);
    let result = sync(&ports, json!({}));
    assert_eq!(result["pushed"].as_array().unwrap().len(), 2, "{result}");
    assert_eq!(shared.pushes().len(), 2);
    ports
        .library
        .write_document(Path::new(PAGE), &page("Topic", "Edited.", true))
        .unwrap();
    shared.reject_pushes(2);
    let refused = call(&ports, "office.library-sync", json!({}));
    assert_eq!(
        refused["error"]["code"], "shared_push_rejected",
        "{refused}"
    );
    assert_eq!(shared.pushes().len(), 4);
    assert_eq!(shared.commits(), 1);
}

#[test]
fn unreachable_and_busy_fail_without_detail_or_state() {
    let (ports, shared, _) = configured();
    shared.set_unreachable(true);
    let response = call(&ports, "office.library-sync", json!({}));
    assert_eq!(response["error"]["code"], "shared_remote_unreachable");
    let message = response["error"]["message"].as_str().unwrap();
    assert!(message.contains("git.example.invalid"), "{message}");
    assert!(!message.contains("team.git"), "{message}");
    assert_eq!(shared.sync_state(), None);
    shared.set_unreachable(false);
    shared.set_busy(true);
    let response = call(&ports, "office.library-sync", json!({}));
    assert_eq!(response["error"]["code"], "library_writer_busy");
    assert_eq!(shared.fetches(), 0);
}

#[test]
fn dry_run_reports_without_state_checkout_proposal_or_push() {
    let (ports, shared, orbit) = configured();
    shared.put_remote(".office-shared.yaml", b"format: 1\n");
    shared.put_remote_symlink("knowledge/domain/dom-link.md", "/etc/passwd");
    let result = sync(&ports, json!({"dry_run":true}));
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["synced_at"], Value::Null);
    assert_eq!(result["pushed"].as_array().unwrap().len(), 2, "{result}");
    assert_eq!(
        result["skipped"],
        json!([{"phase":"pull","path":"knowledge/domain/dom-link.md","id":"dom-link","reason":"shared_file_linked"}])
    );
    assert!(shared.pushes().is_empty());
    assert_eq!(shared.sync_state(), None);
    assert_eq!(shared.checked_out(), None);
    assert!(orbit.calls().is_empty());
}

#[test]
fn a_non_empty_branch_without_the_layout_marker_is_refused() {
    let (ports, shared, _) = configured();
    shared.put_remote("README.md", b"not a shared library\n");
    let response = call(&ports, "office.library-sync", json!({}));
    assert_eq!(
        response["error"]["code"], "shared_layout_invalid",
        "{response}"
    );
    assert!(shared.pushes().is_empty());
}

#[test]
fn changing_the_member_starts_fresh() {
    let (ports, shared, _) = configured();
    sync(&ports, json!({}));
    ports
        .library
        .set_setting(
            "shared",
            json!({"remote":"https://git.example.invalid/team.git","branch":"main","member":"me-two"}),
        )
        .unwrap();
    let result = sync(&ports, json!({}));
    assert_eq!(shared.resets(), 1);
    assert!(
        result["pushed"].as_array().unwrap().contains(&json!(
            "members/me-two/sources/2026/10/src-20261028-call/source.md"
        )),
        "{result}"
    );
    assert_eq!(shared.sync_state().unwrap().member, "me-two");
}
