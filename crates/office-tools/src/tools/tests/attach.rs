//! `office.attach` through dispatch against the fake Library and the
//! recording Orbit fake (STD-04): policy order, the limit of 8, the Library
//! commit, model views on every copy, and refusals before any copy.

use std::path::Path;

use office_core::Error;
use office_core::ports::{Library, Ports};
use office_testkit::fakes::{FakeClock, FakeLibrary, FakeModels, FakeOrbit, FakeStaging};
use serde_json::{Value, json};

const TASK: &str = "T-1";
const SOURCE: &str = "src-20261028-acme-qbr";

struct Fixture {
    ports: Ports<'static>,
    orbit: FakeOrbit,
    staging: FakeStaging,
}

fn page(id: &str, front: &str, body: &str) -> (String, String) {
    let layer = if id.starts_with("rb-") {
        "runbooks"
    } else {
        "domain"
    };
    (
        format!("knowledge/{layer}/{id}.md"),
        format!("---\nid: {id}\n{front}\n---\n{body}"),
    )
}

fn source(participants: &str, body: &str) -> (String, String) {
    (
        format!("sources/2026/10/{SOURCE}/source.md"),
        format!(
            "---\nid: {SOURCE}\nkind: meeting\ntitle: Acme QBR\noccurred_at: 2026-10-28T14:00:00-04:00\nparticipants: [{participants}]\n---\n{body}"
        ),
    )
}

/// A Library with the bundled templates and `documents`, and an Orbit fake
/// that knows one task with `tags`.
fn fixture(documents: &[(String, String)], tags: &[&str]) -> Fixture {
    let library = FakeLibrary::with_bundled_templates();
    for (path, text) in documents {
        library
            .write_document(Path::new(path), text)
            .expect("seed document");
    }
    let staging = library.staging();
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({
        "id": TASK,
        "title": "Prepare the quarterly review",
        "description": "Draft the review deck.",
        "tags": tags,
    })));
    for _ in 0..10 {
        orbit.push_response(Ok(json!({ "ok": true })));
    }
    Fixture {
        ports: Ports {
            library: Box::new(library),
            models: Box::new(FakeModels::new()),
            orbit: Box::new(orbit.clone()),
            clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        },
        orbit,
        staging,
    }
}

fn attach(fixture: &Fixture, input: Value) -> Value {
    let raw = office_testkit::envelope::request("office.attach", input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, &fixture.ports))
        .expect("response is JSON")
}

/// `(id, reason)` for each attached file, in selection order.
fn attached(response: &Value) -> Vec<(String, String)> {
    assert_eq!(response["ok"], json!(true), "{response}");
    response["output"]["attached"]
        .as_array()
        .expect("attached")
        .iter()
        .map(|file| {
            (
                file["id"].as_str().expect("id").to_string(),
                file["reason"].as_str().expect("reason").to_string(),
            )
        })
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(id, reason)| ((*id).to_string(), (*reason).to_string()))
        .collect()
}

/// `(artifact path, staged text)` for every artifact put, in call order.
fn puts(fixture: &Fixture) -> Vec<(String, String)> {
    fixture
        .orbit
        .calls()
        .iter()
        .filter(|call| call.tool == "orbit.task.artifact.put")
        .map(|call| {
            assert_eq!(call.input["id"], json!(TASK));
            let staged = call.input["source_path"].as_str().expect("source_path");
            let text = fixture.staging.text(staged).expect("put a staged file");
            (call.input["path"].as_str().expect("path").to_string(), text)
        })
        .collect()
}

fn user_page() -> (String, String) {
    page(
        "dom-user",
        "template: user\ntitle: Dee\nrefs:\n  - { label: Work email, contact: dee@example.com }\n  - { label: Tracker, account: \"jira:OFF\" }",
        "## Name\nDee\n\n## Role\nOperations lead.\n",
    )
}

#[test]
fn always_attaches_current_always_pages_only() {
    let fixture = fixture(
        &[
            user_page(),
            page(
                "dom-systems",
                "template: systems\ntitle: Systems\nstatus: stale",
                "## Tools\nJira.\n",
            ),
            page(
                "dom-goals",
                "template: goals\ntitle: Goals",
                "## Priorities\nShip.\n",
            ),
        ],
        &[],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(attached(&response), pairs(&[("dom-user", "always")]));
}

#[test]
fn participants_match_person_pages_by_title_or_alias() {
    let fixture = fixture(
        &[
            source("J. Park (Acme), Daniel Kim", "[00:00:01] Jane: Hello.\n"),
            page(
                "dom-person-jane-park",
                "template: person\ntitle: Jane Park\naliases: [J. Park]",
                "## Role\nBuyer.\n",
            ),
            page(
                "dom-person-daniel-kim",
                "template: person\ntitle: Daniel Kim",
                "## Role\nOwner.\n",
            ),
            page(
                "dom-person-bo-lee",
                "template: person\ntitle: Bo Lee\naliases: [Bo]",
                "## Role\nAbsent.\n",
            ),
        ],
        &[&format!("source:{SOURCE}")],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(
        attached(&response),
        pairs(&[
            ("dom-person-daniel-kim", "participants"),
            ("dom-person-jane-park", "participants"),
            (SOURCE, "source"),
        ])
    );
}

#[test]
fn tagged_matches_subject_pages_by_the_tasks_tags() {
    let fixture = fixture(
        &[
            page(
                "dom-client-acme",
                "template: client\ntitle: Acme\ntags: [client:acme]",
                "## Overview\nAcme.\n",
            ),
            page(
                "dom-client-beta",
                "template: client\ntitle: Beta\ntags: [client:beta]",
                "## Overview\nBeta.\n",
            ),
            page(
                "dom-project-acme-renewal",
                "template: project\ntitle: Acme renewal\ntags: [project:acme-renewal]",
                "## Status\nOpen.\n\n## Background\nRenews.\n",
            ),
        ],
        &["client:acme", "project:acme-renewal"],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(
        attached(&response),
        pairs(&[
            ("dom-client-acme", "tagged"),
            ("dom-project-acme-renewal", "tagged"),
        ])
    );
}

#[test]
fn matched_attaches_the_glossary_when_the_source_uses_a_term() {
    let glossary = page(
        "dom-glossary",
        "template: glossary\ntitle: Glossary",
        "## Terms\n### MSA\nMaster services agreement.\n",
    );
    let using = fixture(
        &[
            glossary.clone(),
            source("Jane", "[00:00:01] Jane: The MSA renews.\n"),
        ],
        &[&format!("source:{SOURCE}")],
    );
    assert_eq!(
        attached(&attach(&using, json!({ "task": TASK }))),
        pairs(&[("dom-glossary", "matched"), (SOURCE, "source")])
    );
    // "msas" is not the whole word "msa".
    let not_using = fixture(
        &[glossary, source("Jane", "[00:00:01] Jane: Several MSAs.\n")],
        &[&format!("source:{SOURCE}")],
    );
    assert_eq!(
        attached(&attach(&not_using, json!({ "task": TASK }))),
        pairs(&[(SOURCE, "source")])
    );
}

#[test]
fn policies_come_first_in_order_and_everything_stays_within_eight() {
    let mut documents = vec![
        user_page(),
        page(
            "dom-standing-directions",
            "template: standing-directions\ntitle: Standing directions",
            "## Directions\n- Always draft.\n",
        ),
        source("Jane Park (Acme)", "[00:00:01] Jane: QBR next week.\n"),
        page(
            "dom-person-jane-park",
            "template: person\ntitle: Jane Park",
            "## Role\nBuyer.\n",
        ),
        page(
            "dom-glossary",
            "template: glossary\ntitle: Glossary",
            "## Terms\n### QBR\nQuarterly business review.\n",
        ),
        page(
            "rb-client-qbr-prep",
            "template: runbook\ntitle: QBR prep",
            "## Inputs\nDeck.\n\n## Steps\nDraft.\n\n## Done when\nSent.\n",
        ),
    ];
    for n in 1..=4 {
        documents.push(page(
            &format!("dom-topic-acme-{n}"),
            &format!("template: topic\ntitle: Topic {n}\ntags: [client:acme]"),
            "## Overview\nx\n",
        ));
    }
    let fixture = fixture(
        &documents,
        &[
            "client:acme",
            &format!("source:{SOURCE}"),
            "runbook:rb-client-qbr-prep",
        ],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(
        attached(&response),
        pairs(&[
            ("dom-standing-directions", "always"),
            ("dom-user", "always"),
            ("dom-person-jane-park", "participants"),
            ("dom-topic-acme-1", "tagged"),
            ("dom-topic-acme-2", "tagged"),
            ("dom-topic-acme-3", "tagged"),
            ("dom-topic-acme-4", "tagged"),
            ("dom-glossary", "matched"),
        ])
    );
    // The runbook and the source come after the policies and are cut.
    assert_eq!(
        response["output"]["omitted"],
        json!([
            { "id": "rb-client-qbr-prep", "reason": "limit" },
            { "id": SOURCE, "reason": "limit" },
        ])
    );
    let copies = puts(&fixture);
    let context: Vec<&str> = copies
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| path.starts_with("context/"))
        .collect();
    assert_eq!(context.len(), 8);
    assert_eq!(copies.len(), 9, "eight copies and the manifest");
}

#[test]
fn runbooks_sources_and_series_examples_follow_the_policies() {
    let fixture = fixture(
        &[
            source("Jane", "[00:00:01] Jane: Hello.\n"),
            page(
                "rb-client-qbr-prep",
                "template: runbook\ntitle: QBR prep",
                "## Inputs\nDeck.\n\n## Steps\nDraft.\n\n## Done when\nSent.\n",
            ),
            (
                "artifacts/2026/07/art-20260721-qbr-deck/artifact.md".into(),
                "---\nid: art-20260721-qbr-deck\nkind: doc\ntitle: Q2 deck\nseries: qbr-deck\n---\nOlder.\n".into(),
            ),
            (
                "artifacts/2026/10/art-20261021-qbr-deck/artifact.md".into(),
                "---\nid: art-20261021-qbr-deck\nkind: doc\ntitle: Q3 deck\nseries: qbr-deck\n---\nNewer.\n".into(),
            ),
        ],
        &[&format!("source:{SOURCE}"), "series:qbr-deck"],
    );
    let response = attach(
        &fixture,
        json!({ "task": TASK, "text": "Follow rb-client-qbr-prep." }),
    );
    assert_eq!(
        attached(&response),
        pairs(&[
            ("rb-client-qbr-prep", "runbook"),
            (SOURCE, "source"),
            ("art-20261021-qbr-deck", "example"),
            ("art-20260721-qbr-deck", "example"),
        ])
    );
    let copies = puts(&fixture);
    assert_eq!(
        copies[1],
        (
            format!("context/sources/2026/10/{SOURCE}/source.md"),
            source("Jane", "[00:00:01] Jane: Hello.\n").1
        ),
        "a source is copied as its file"
    );
}

#[test]
fn copies_go_under_context_with_the_library_commit_recorded() {
    let fixture = fixture(&[user_page()], &[]);
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(response["ok"], json!(true), "{response}");
    let output = &response["output"];
    let commit = output["library_commit"].as_str().expect("a commit");
    assert_eq!(output["task"], json!(TASK));
    assert_eq!(output["manifest"], json!("office-attach.json"));
    assert_eq!(
        output["attached"][0]["artifact_path"],
        json!("context/knowledge/domain/dom-user.md")
    );

    let calls = fixture.orbit.calls();
    assert_eq!(calls[0].tool, "orbit.task.show");
    assert_eq!(calls[0].input["id"], json!(TASK));
    let copies = puts(&fixture);
    assert_eq!(copies[0].0, "context/knowledge/domain/dom-user.md");
    let (path, manifest) = &copies[1];
    assert_eq!(path, "office-attach.json");
    let manifest: Value = serde_json::from_str(manifest).expect("manifest is JSON");
    assert_eq!(manifest["library_commit"], json!(commit));
    assert_eq!(manifest["task"], json!(TASK));
    assert_eq!(manifest["attached"], output["attached"]);
    assert!(
        fixture.staging.live().is_empty(),
        "staging files are removed"
    );
}

#[test]
fn every_attached_page_is_its_model_view_without_contact_values() {
    let fixture = fixture(
        &[
            user_page(),
            source("Jane Park", "[00:00:01] Jane: Hello.\n"),
            page(
                "dom-person-jane-park",
                "template: person\ntitle: Jane Park\nrefs:\n  - { label: Work email, contact: jane@acme.example }\n  - { label: Mobile, contact: \"+1 415 555 0100\" }",
                "## Role\nBuyer. Old note: jane@acme.example\n",
            ),
        ],
        &[&format!("source:{SOURCE}")],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(attached(&response).len(), 3, "{response}");
    let copies = puts(&fixture);
    let pages: Vec<&(String, String)> = copies
        .iter()
        .filter(|(path, _)| path.starts_with("context/knowledge/"))
        .collect();
    assert_eq!(pages.len(), 2);
    for (path, text) in &copies {
        for value in ["dee@example.com", "jane@acme.example", "555 0100"] {
            assert!(!text.contains(value), "{path} carries {value}");
        }
    }
    // Contacts keep their labels; other pointers keep their values.
    let (user, jane) = (&pages[0].1, &pages[1].1);
    assert!(
        jane.contains("{ label: \"Work email\", contact: null }"),
        "{jane}"
    );
    assert!(
        jane.contains("{ label: \"Mobile\", contact: null }"),
        "{jane}"
    );
    assert!(
        user.contains("{ label: \"Tracker\", account: \"jira:OFF\" }"),
        "{user}"
    );
}

#[test]
fn private_pages_and_documents_are_withheld() {
    let fixture = fixture(
        &[
            user_page(),
            page(
                "dom-standing-directions",
                "template: standing-directions\ntitle: Standing directions\nsensitivity: private",
                "## Directions\n- Never send.\n",
            ),
        ],
        &[],
    );
    let response = attach(&fixture, json!({ "task": TASK }));
    assert_eq!(attached(&response), pairs(&[("dom-user", "always")]));
    assert_eq!(
        response["output"]["omitted"],
        json!([{ "id": "dom-standing-directions", "reason": "private" }])
    );
}

#[test]
fn an_unknown_task_is_refused_before_any_copy() {
    let library = FakeLibrary::with_bundled_templates();
    let (path, text) = user_page();
    library
        .write_document(Path::new(&path), &text)
        .expect("seed");
    let staging = library.staging();
    let orbit = FakeOrbit::default();
    orbit.push_response(Err(Error::OrbitRefused {
        code: "task_not_found".into(),
    }));
    let fixture = Fixture {
        ports: Ports {
            library: Box::new(library),
            models: Box::new(FakeModels::new()),
            orbit: Box::new(orbit.clone()),
            clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        },
        orbit,
        staging,
    };
    let response = attach(&fixture, json!({ "task": "T-404" }));
    assert_eq!(
        response["error"]["code"],
        json!("task_not_found"),
        "{response}"
    );
    let tools: Vec<&str> = fixture.orbit.calls().iter().map(|call| call.tool).collect();
    assert_eq!(tools, ["orbit.task.show"]);
    assert!(fixture.staging.written().is_empty());
}

#[test]
fn a_dry_run_previews_without_reading_a_task_or_copying() {
    let library = FakeLibrary::with_bundled_templates();
    for (path, text) in [
        user_page(),
        page(
            "dom-client-acme",
            "template: client\ntitle: Acme\ntags: [client:acme]",
            "## Overview\nAcme.\n",
        ),
    ] {
        library
            .write_document(Path::new(&path), &text)
            .expect("seed");
    }
    let staging = library.staging();
    let orbit = FakeOrbit::default();
    let fixture = Fixture {
        ports: Ports {
            library: Box::new(library),
            models: Box::new(FakeModels::new()),
            orbit: Box::new(orbit.clone()),
            clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
        },
        orbit,
        staging,
    };
    let response = attach(
        &fixture,
        json!({ "dry_run": true, "tags": ["client:acme"] }),
    );
    assert_eq!(
        attached(&response),
        pairs(&[("dom-user", "always"), ("dom-client-acme", "tagged")])
    );
    assert_eq!(response["output"]["task"], Value::Null);
    assert_eq!(response["output"]["manifest"], Value::Null);
    assert!(fixture.orbit.calls().is_empty());
    assert!(fixture.staging.written().is_empty());
}

#[test]
fn malformed_input_is_refused() {
    let fixture = fixture(&[user_page()], &[]);
    for input in [
        json!({}),
        json!({ "text": "Acme" }),
        json!({ "dry_run": true }),
        json!({ "task": "" }),
        json!({ "task": TASK, "tags": "client:acme" }),
        json!({ "task": TASK, "dry_run": "yes" }),
        json!({ "task": TASK, "limit": 3 }),
    ] {
        let response = attach(&fixture, input.clone());
        assert_eq!(
            response["error"]["code"],
            json!("invalid_argument"),
            "{input}"
        );
    }
    assert!(fixture.orbit.calls().is_empty());
}
