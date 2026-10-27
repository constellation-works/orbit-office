//! `office.knowledge-write` through dispatch against the fake Library
//! (STD-04): every refusal writes and commits nothing; a valid page commits
//! once; template, contact, length and policy rules.

use std::path::Path;

use office_core::ports::{Library, LibraryText, Ports};
use office_testkit::fakes::FakeLibrary;
use serde_json::{Value, json};

const SOURCE: &str = "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\n---\n[00:10:00] Jane: Renewal is in March.\n\n[00:12:31] Jane: Price matters most.\nLine five.\n";

fn ports(library: FakeLibrary) -> Ports<'static> {
    library
        .write_document(
            Path::new("sources/2026/10/src-20261028-acme-qbr/source.md"),
            SOURCE,
        )
        .expect("seed source");
    Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

fn write(ports: &Ports<'_>, markdown: &str) -> Value {
    let raw = office_testkit::envelope::request(
        "office.knowledge-write",
        json!({ "markdown": markdown }),
        json!({}),
    );
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

#[test]
fn a_commented_quoted_page_stays_retrievable_by_canonical_id_and_tag() {
    let ports = ports(FakeLibrary::initialized());
    // An unrelated legacy document (duplicate title) must still index.
    ports
        .library
        .write_document(
            Path::new("sources/2026/10/src-20261028-legacy/source.md"),
            "---\nid: src-20261028-legacy # source\ntitle: First\ntitle: Legacy\ntags: [\"client:acme,inc\"]\n---\nLegacy body\n",
        )
        .expect("seed legacy");

    let markdown = "---\n\
        id: dom-person-ada # identity\n\
        type: domain # domain | runbook\n\
        title: \"Ada \\\"Ada\\\" Lovelace\" # name\n\
        sensitivity: private # privacy\n\
        tags: [\"client:acme,inc\"] # tags\n\
        ---\nAda decides.\n";
    let written = write(&ports, markdown);
    assert_eq!(written["ok"], json!(true), "{written}");
    assert_eq!(written["output"]["id"], json!("dom-person-ada"));

    let shown = call(
        &ports,
        "office.knowledge-show",
        json!({ "id": "dom-person-ada" }),
    );
    assert_eq!(shown["ok"], json!(true), "{shown}");
    assert_eq!(
        shown["output"]["page"]["title"],
        json!("Ada \"Ada\" Lovelace")
    );
    assert_eq!(shown["output"]["page"]["tags"], json!(["client:acme,inc"]));
    assert_eq!(shown["output"]["page"]["sensitivity"], json!("private"));

    let reindexed = call(&ports, "office.reindex", json!({}));
    assert_eq!(reindexed["ok"], json!(true), "{reindexed}");
    assert!(
        reindexed["output"]["indexed_documents"]
            .as_u64()
            .expect("count")
            >= 3,
        "{reindexed}"
    );

    let found = call(
        &ports,
        "office.search",
        json!({ "query": "Ada", "tag": "client:acme,inc" }),
    );
    assert_eq!(found["ok"], json!(true), "{found}");
    let ids: Vec<&str> = found["output"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .filter_map(|hit| hit["id"].as_str())
        .collect();
    assert_eq!(ids, ["dom-person-ada"], "{found}");
}

fn page_count(ports: &Ports<'_>) -> usize {
    ports.library.knowledge_pages().expect("pages").len()
}

/// Refused with `code`, and the Library's knowledge layer is unchanged.
fn assert_refused(ports: &Ports<'_>, markdown: &str, code: &str) {
    let before = page_count(ports);
    let response = write(ports, markdown);
    assert_eq!(response["ok"], json!(false), "{code}: {response}");
    assert_eq!(response["error"]["code"], json!(code), "{response}");
    assert_eq!(page_count(ports), before, "{code} wrote nothing");
}

fn domain(id: &str, extra: &str, body: &str) -> String {
    format!("---\nid: {id}\ntitle: A page\n{extra}---\n{body}")
}

#[test]
fn a_cite_to_a_missing_source_or_anchor_is_refused_and_a_valid_page_commits_once() {
    let ports = ports(FakeLibrary::initialized());
    for cite in [
        "src-20261028-missing#t=00:12:31",
        "src-20261028-acme-qbr",
        "src-20261028-acme-qbr#t=00:00:01",
        "src-20261028-acme-qbr#L5",
        "src-20261028-acme-qbr#frame=00:12:31",
        "dom-absent",
        "not a citation",
    ] {
        let page = domain("dom-acme", &format!("cites: [\"{cite}\"]\n"), "Body.\n");
        assert_refused(&ports, &page, "unresolved_cite");
    }
    let inline = domain(
        "dom-acme",
        "",
        "Claim [src-20261028-acme-qbr#t=09:00:00].\n",
    );
    assert_refused(&ports, &inline, "unresolved_cite");

    let valid = domain(
        "dom-acme",
        "cites: [src-20261028-acme-qbr#t=00:12:31, src-20261028-acme-qbr#L1-4]\n",
        "Price matters [src-20261028-acme-qbr#t=00:11:00; src-20261028-acme-qbr#L2].\n",
    );
    let response = write(&ports, &valid);
    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(
        response["output"],
        json!({"id": "dom-acme", "type": "domain", "path": "knowledge/domain/dom-acme.md", "created": true})
    );
    let fake = ports.library.as_ref();
    assert_eq!(page_count(&ports), 1);
    let history = fake
        .history(Path::new("knowledge/domain/dom-acme.md"))
        .expect("history");
    assert_eq!(history.len(), 1, "one commit");

    let again = write(&ports, &valid);
    assert_eq!(again["output"]["created"], json!(false), "{again}");
}

#[test]
fn template_rules_are_each_refused_and_a_page_without_template_meets_base_rules_only() {
    let library = FakeLibrary::with_bundled_templates();
    // A hand-written page already uses the one-per-Library `user` template.
    library
        .write_document(
            Path::new("knowledge/domain/dom-me.md"),
            "---\nid: dom-me\ntemplate: user\ntitle: Me\n---\n## Name\nDee\n",
        )
        .expect("seed");
    let ports = ports(library);

    let cases = [
        (
            "---\nid: dom-person-jane\ntemplate: nobody\ntitle: Jane\n---\n## Role\nBuyer.\n",
            "unknown_template",
        ),
        (
            "---\nid: dom-jane\ntemplate: person\ntitle: Jane\n---\n## Role\nBuyer.\n",
            "page_id_mismatch",
        ),
        (
            "---\nid: dom-user\ntemplate: user\ntitle: Me\n---\n## Name\nDee\n\n## Role\nLead.\n",
            "template_page_exists",
        ),
        (
            "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\n---\n## Role\n\n## History\nMet in 2025.\n",
            "missing_section",
        ),
        (
            "---\nid: dom-person-jane\ntype: runbook\ntemplate: person\ntitle: Jane\n---\n## Role\nBuyer.\n",
            "template_type_mismatch",
        ),
    ];
    for (markdown, code) in cases {
        assert_refused(&ports, markdown, code);
    }

    let person = write(
        &ports,
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\n---\n## Role\nBuyer.\n",
    );
    assert_eq!(person["ok"], json!(true), "{person}");
    // No template: any id shape and no required sections.
    let free = write(&ports, &domain("dom-notes", "", "Just notes.\n"));
    assert_eq!(free["ok"], json!(true), "{free}");
    // Base rules still apply to a free-form page.
    assert_refused(
        &ports,
        "---\nid: dom-notes\ntype: runbook\ntitle: Notes\n---\nx\n",
        "invalid_frontmatter",
    );
}

#[test]
fn a_broken_template_file_is_refused_as_template_invalid() {
    let mut library = FakeLibrary::initialized();
    library.templates = vec![LibraryText {
        path: ".office/templates/person.md".into(),
        markdown: Some("---\ntemplate: person\n---\n".into()),
    }];
    let ports = ports(library);
    assert_refused(
        &ports,
        "---\nid: dom-person-jane\ntemplate: person\ntitle: Jane\n---\n## Role\nBuyer.\n",
        "template_invalid",
    );
}

#[test]
fn contact_details_in_the_body_are_refused_while_aliases_and_account_refs_are_accepted() {
    let ports = ports(FakeLibrary::initialized());
    for body in [
        "Write to jane.park@acme.example about it.\n",
        "Call +1 (555) 123-4567 after noon.\n",
        "Her desk is 020 7946 0958.\n",
    ] {
        let response = write(&ports, &domain("dom-person-jane", "", body));
        assert_eq!(
            response["error"]["code"],
            json!("contact_in_body"),
            "{body}: {response}"
        );
        assert!(
            !response.to_string().contains("acme.example")
                && !response.to_string().contains("4567"),
            "the refusal never echoes the value: {response}"
        );
    }
    assert_eq!(page_count(&ports), 0);

    let accepted = write(
        &ports,
        &domain(
            "dom-person-jane",
            "aliases: [Jane, J. Park]\nrefs:\n  - { label: Tickets, account: \"jira:ACME\" }\n  - label: Email\n    contact: jane.park@acme.example\n",
            "Met on 2026-10-28 at [00:12:31]; budget 1,250,000; ticket ACME-1234567890.\n",
        ),
    );
    assert_eq!(accepted["ok"], json!(true), "{accepted}");
}

#[test]
fn long_pages_and_unsourced_policies_are_refused_and_runbook_submission_is_accepted() {
    let ports = ports(FakeLibrary::initialized());
    let long = "word ".repeat(1_201);
    assert_refused(
        &ports,
        &domain("dom-topic-long", "", &long),
        "page_too_long",
    );
    let at_cap = write(&ports, &domain("dom-topic-cap", "", &"word ".repeat(1_200)));
    assert_eq!(at_cap["ok"], json!(true), "{at_cap}");

    assert_refused(
        &ports,
        &domain("dom-topic-pto", "tags: [policy]\n", "Summary.\n"),
        "policy_without_source",
    );
    assert_refused(
        &ports,
        &domain(
            "dom-topic-leave",
            "refs:\n  - { label: Leave policy, url: \"https://intranet.example/leave\", authoritative: true }\n",
            "## Overview\nx\n\n## Leave [policy]\nTwo weeks.\n",
        ),
        "policy_without_source",
    );
    let sourced = write(
        &ports,
        &domain(
            "dom-topic-leave",
            "refs:\n  - { label: Leave policy, url: \"https://intranet.example/leave\", authoritative: true, as_of: 2025-11-03 }\n",
            "## Leave [policy]\nTwo weeks.\n",
        ),
    );
    assert_eq!(sourced["ok"], json!(true), "{sourced}");

    let runbook = write(
        &ports,
        "---\nid: rb-request-ad-group\ntitle: Request an AD group\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: gateway\n  connection: servicenow\n---\n## Inputs\nThe group.\n",
    );
    assert_eq!(runbook["ok"], json!(true), "{runbook}");
    assert_eq!(
        runbook["output"]["path"],
        json!("knowledge/runbooks/rb-request-ad-group.md")
    );
    assert_refused(
        &ports,
        "---\nid: rb-request-pto\ntitle: Request PTO\nsubmission:\n  system: Workday\n  via: gateway\n---\nx\n",
        "invalid_frontmatter",
    );
}

#[test]
fn malformed_input_is_refused_before_any_write() {
    let ports = ports(FakeLibrary::initialized());
    for markdown in [
        "no frontmatter",
        "---\ntitle: No id\n---\n",
        "---\nid: dom-a\ntitle: A\nsecret_token: x\n---\n",
        "---\nid: dom-a\ntitle: A\nrefs:\n  - { label: Two, url: \"https://a.example\", path: /tmp/x }\n---\n",
        "---\nid: dom-a\ntitle: A\nrefs:\n  - { label: Creds, url: \"https://user:pw@a.example\" }\n---\n",
        "---\nid: dom-a\ntitle: A\nrefs:\n  - { label: Bad, account: ACME }\n---\n",
    ] {
        assert_refused(&ports, markdown, "invalid_frontmatter");
    }
    let response = write(&ports, "");
    assert_eq!(response["error"]["code"], json!("invalid_frontmatter"));
    let raw = office_testkit::envelope::request(
        "office.knowledge-write",
        json!({ "markdown": "x", "path": "knowledge/domain/x.md" }),
        json!({}),
    );
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    let response = office_testkit::envelope::response(&crate::serve(&request, &ports))
        .expect("response is JSON");
    assert_eq!(response["error"]["code"], json!("invalid_argument"));
}
