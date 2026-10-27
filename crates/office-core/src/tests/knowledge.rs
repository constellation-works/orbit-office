//! Knowledge page parsing, base rules, anchors and the model view through
//! the public domain API.

use office_common::Sensitivity;

use crate::Error;
use crate::domain::knowledge::{
    DocumentKind, MAX_PAGE_WORDS, Page, PageStatus, Performable, RefTarget, SubmissionVia,
    parse_citation, resolve_anchor,
};

// The example in specs/library.md, "Knowledge pages", plus `as_of`.
const SPEC_PAGE: &str = r#"---
id: dom-project-acme-renewal   # dom- | rb-
type: domain                   # domain | runbook
template: project              # optional; see knowledge-templates.md
title: Acme renewal
aliases: [Acme renewal, the renewal]
summary: Acme's contract renews in March; Jane Park decides; price sensitivity high.
tags: [project:acme-renewal, client:acme]
cites:
  - src-20261028-acme-qbr#t=00:12:31
  - src-20261024-slack-pricing#L14-20
refs:                          # pointers, domain pages only
  - { label: Master services agreement, path: ~/Documents/Acme/MSA-2025.pdf }
  - { label: Renewal tickets, account: "jira:ACME" }
  - { label: QBR deck template, artifact: art-20261021-qbr-template }
  - { label: Jane Park, contact: "jane.park@acme.example" }
related: [rb-client-qbr-prep, dom-client-acme, dom-person-jane-park]
updated_at: 2026-10-28T15:10:00-04:00
last_verified: 2026-10-28
status: current                # current | stale | retired
---
## Status
Renews in March [src-20261028-acme-qbr#t=00:12:31].
"#;

fn page(markdown: &str) -> Page {
    Page::from_markdown(markdown).expect("valid page")
}

fn code(result: crate::Result<impl std::fmt::Debug>) -> &'static str {
    result.expect_err("refused").code()
}

#[test]
fn the_spec_example_parses_with_every_field() {
    let page = page(SPEC_PAGE);
    assert_eq!(page.id, "dom-project-acme-renewal");
    assert_eq!(page.kind, DocumentKind::Domain);
    assert_eq!(page.template.as_deref(), Some("project"));
    assert_eq!(page.aliases, ["Acme renewal", "the renewal"]);
    assert_eq!(
        page.summary,
        "Acme's contract renews in March; Jane Park decides; price sensitivity high."
    );
    assert_eq!(page.tags, ["project:acme-renewal", "client:acme"]);
    assert_eq!(page.cites.len(), 2);
    let targets: Vec<RefTarget> = page.refs.iter().map(|r| r.target).collect();
    assert_eq!(
        targets,
        [
            RefTarget::Path,
            RefTarget::Account,
            RefTarget::Artifact,
            RefTarget::Contact
        ]
    );
    assert_eq!(page.refs[1].value, "jira:ACME");
    assert_eq!(page.related.len(), 3);
    assert_eq!(
        page.updated_at.as_deref(),
        Some("2026-10-28T15:10:00-04:00")
    );
    assert_eq!(page.last_verified.as_deref(), Some("2026-10-28"));
    assert_eq!(page.status, PageStatus::Current);
    assert_eq!(page.sensitivity, Sensitivity::Normal);
    assert_eq!(page.path(), "knowledge/domain/dom-project-acme-renewal.md");
    page.check_base_rules().expect("base rules hold");
    let cited: Vec<String> = page
        .citations()
        .expect("citations parse")
        .into_iter()
        .map(|c| c.text)
        .collect();
    assert_eq!(
        cited,
        [
            "src-20261028-acme-qbr#t=00:12:31",
            "src-20261024-slack-pricing#L14-20"
        ],
        "the inline citation repeats a frontmatter one"
    );
}

#[test]
fn the_model_view_keeps_contact_labels_and_drops_their_values() {
    let page = page(&SPEC_PAGE.replace(
        "Renews in March",
        "Renews in March; ask jane.park@acme.example or +1 555 123 4567",
    ));
    let view = page.model_view();
    let contact = &view.refs[3];
    assert_eq!(contact.label, "Jane Park");
    assert_eq!(contact.target, RefTarget::Contact);
    assert_eq!(contact.value, None);
    assert_eq!(view.refs[1].value.as_deref(), Some("jira:ACME"));
    assert!(
        view.body
            .contains("ask [contact withheld] or [contact withheld]")
    );
    let markdown = view.to_markdown();
    assert!(!markdown.contains("acme.example"), "{markdown}");
    assert!(!markdown.contains("4567"), "{markdown}");
    assert!(markdown.contains(r#"{ label: "Jane Park", contact: null }"#));
    let json = serde_json::to_value(&view).expect("view serializes");
    assert_eq!(json["refs"][3]["value"], serde_json::Value::Null);
    assert_eq!(json["type"], "domain");
}

#[test]
fn contact_detection_ignores_timestamps_dates_ids_and_amounts() {
    let ok = "---\nid: dom-a\ntitle: A\n---\nAt [00:12:31] on 2026-10-28 to 2026-11-03, see [src-20261028-acme-qbr#L14-20], ACME-1234567890, budget $1,250,000 and v2.10.3.\n";
    page(ok).check_base_rules().expect("no contact detail");
    for body in [
        "mail jane@acme.example",
        "call (555) 123-4567",
        "call 555.123.4567",
        "call +44 20 7946 0958",
        "call 5551234567",
    ] {
        let markdown = format!("---\nid: dom-a\ntitle: A\n---\n{body}\n");
        assert!(
            matches!(
                page(&markdown).check_base_rules(),
                Err(Error::ContactInBody { .. })
            ),
            "{body}"
        );
    }
}

#[test]
fn word_cap_policy_and_layer_rules() {
    let at_cap = format!(
        "---\nid: dom-a\ntitle: A\n---\n{}",
        "w ".repeat(MAX_PAGE_WORDS)
    );
    page(&at_cap).check_base_rules().expect("at the cap");
    let over = format!("{at_cap}w\n");
    assert_eq!(code(page(&over).check_base_rules()), "page_too_long");

    let tagged = "---\nid: dom-topic-pto\ntitle: PTO\ntags: [policy]\nas_of: 2025-11-03\nrefs:\n  - { label: PTO policy, url: \"https://hr.example/pto\", authoritative: true }\n---\nSummary.\n";
    page(tagged)
        .check_base_rules()
        .expect("page-level as_of counts");
    let undated = tagged.replace("as_of: 2025-11-03\n", "");
    assert_eq!(
        code(page(&undated).check_base_rules()),
        "policy_without_source"
    );
    let section = "---\nid: dom-topic-hr\ntitle: HR\n---\n## Leave [policy]\nTwo weeks.\n";
    assert_eq!(
        code(page(section).check_base_rules()),
        "policy_without_source"
    );
    let fenced = "---\nid: dom-topic-hr\ntitle: HR\n---\n```\n## Leave [policy]\n```\n";
    page(fenced)
        .check_base_rules()
        .expect("a fenced heading is not a section");

    let runbook = page(
        "---\nid: rb-request-ad-group\ntype: runbook\ntitle: AD group\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: draft\n---\n## Inputs\nx\n",
    );
    assert_eq!(runbook.kind, DocumentKind::Runbook);
    assert_eq!(runbook.performable, Some(Performable::Agent));
    let submission = runbook.submission.as_ref().expect("submission");
    assert_eq!(submission.via, SubmissionVia::Draft);
    assert_eq!(submission.connection, None);
    runbook
        .check_base_rules()
        .expect("runbook fields on a runbook");
    assert_eq!(runbook.path(), "knowledge/runbooks/rb-request-ad-group.md");

    let domain_performable = "---\nid: dom-a\ntitle: A\nperformable: agent\n---\n";
    assert_eq!(
        code(page(domain_performable).check_base_rules()),
        "invalid_frontmatter"
    );
    let runbook_refs = "---\nid: rb-a\ntitle: A\nrefs: [{ label: X, path: /x }]\n---\n";
    assert_eq!(
        code(page(runbook_refs).check_base_rules()),
        "invalid_frontmatter"
    );
}

#[test]
fn frontmatter_refusals_name_the_field() {
    for (markdown, field) in [
        ("---\nid: dom-a\ntitle: A\nowner: me\n---\n", "owner"),
        ("---\nid: Dom-A\ntitle: A\n---\n", "id"),
        ("---\nid: src-a\ntitle: A\n---\n", "id"),
        ("---\nid: dom-a\n---\n", "title"),
        ("---\nid: dom-a\ntitle: A\nstatus: draft\n---\n", "status"),
        (
            "---\nid: dom-a\ntitle: A\nlast_verified: yesterday\n---\n",
            "last_verified",
        ),
        ("---\nid: dom-a\ntitle: A\ntitle: B\n---\n", "title"),
        ("---\nid: dom-a\ntitle: &anchor A\n---\n", "title"),
        ("---\nid: dom-a\ntitle: A\ntags: [a, [b]]\n---\n", "tags"),
        (
            "---\nid: dom-a\ntitle: A\nrelated: [src-x]\n---\n",
            "related",
        ),
        (
            "---\nid: dom-a\ntitle: A\nsubmission:\n  system: X\n  via: email\n---\n",
            "submission",
        ),
    ] {
        match Page::from_markdown(markdown) {
            Err(Error::InvalidFrontmatter { field: got, .. }) => {
                assert_eq!(got, field, "{markdown}");
            }
            other => panic!("{markdown}: {other:?}"),
        }
    }
}

#[test]
fn timestamp_overflow_is_refused_by_citation_resolution() {
    for stamp in ["1152921504606846976:00:00", "5124095576030431:00:16"] {
        assert!(resolve_anchor("[00:00:00] Jane: zero", &format!("t={stamp}")).is_err());
        assert!(resolve_anchor(&format!("[{stamp}] Jane: overflow"), "t=00:00:00").is_err());
    }
    for stamp in ["100:00:00", "5124095576040:25:51", "5124095576030431:00:15"] {
        assert!(resolve_anchor(&format!("[{stamp}] Jane: valid"), &format!("t={stamp}")).is_ok());
    }
}

#[test]
fn anchors_resolve_against_the_source_body() {
    let meeting = "[00:10:00] Jane: one\n\n[00:12:31] Dee: two\n";
    assert!(resolve_anchor(meeting, "t=00:12:31").is_ok());
    assert!(resolve_anchor(meeting, "t=00:11:00").is_ok());
    assert!(resolve_anchor(meeting, "t=00:09:59").is_err());
    assert!(resolve_anchor(meeting, "t=00:12:32").is_err());
    assert!(resolve_anchor("no turns", "t=00:00:00").is_err());
    assert!(resolve_anchor(meeting, "L1-3").is_ok());
    assert!(resolve_anchor(meeting, "L3").is_ok());
    assert!(resolve_anchor(meeting, "L4").is_err());
    assert!(resolve_anchor(meeting, "L0").is_err());
    assert!(resolve_anchor(meeting, "L3-1").is_err());
    let deck = "## Slide 1: Intro\nx\n\n## Slide 2\ny\n";
    assert!(resolve_anchor(deck, "slide=2").is_ok());
    assert!(resolve_anchor(deck, "slide=3").is_err());
    assert!(resolve_anchor("# Page 4\ntext\n", "page=4").is_ok());
    assert!(resolve_anchor(meeting, "frame=00:12:31").is_err());
    assert!(resolve_anchor(meeting, "nonsense").is_err());

    let cite = parse_citation("src-20261028-acme-qbr#t=00:12:31").expect("cite");
    assert_eq!(cite.kind, DocumentKind::Source);
    assert_eq!(cite.anchor.as_deref(), Some("t=00:12:31"));
    assert!(parse_citation("art-20261021-qbr-template").is_some());
    assert!(parse_citation("Jane Park").is_none());
    assert!(parse_citation("src-x#").is_none());
}

#[test]
fn index_records_decode_comments_quoted_scalars_and_quoted_list_items() {
    use crate::domain::knowledge::DocumentRecord;
    use std::path::Path;

    let record = DocumentRecord::from_markdown(
        Path::new("knowledge/domain/dom-person-ada.md"),
        "---\n\
         id: dom-person-ada # identity\n\
         type: domain # domain | runbook\n\
         title: \"Ada \\\"Ada\\\" Lovelace\" # name\n\
         summary: 'It''s Ada' # summary\n\
         sensitivity: private # privacy\n\
         tags: [\"client:acme,inc\", plain] # tags\n\
         cites: [\"src-20261028-acme-qbr#L1-2\"]\n\
         related:\n\
         \x20 - \"dom-client-acme,inc\" # related\n\
         ---\nBody\n",
    );
    assert_eq!(record.id, "dom-person-ada");
    assert_eq!(record.title, "Ada \"Ada\" Lovelace");
    assert_eq!(record.summary, "It's Ada");
    assert_eq!(record.sensitivity, "private");
    assert_eq!(record.tags, ["client:acme,inc", "plain"]);
    assert_eq!(record.cites, ["src-20261028-acme-qbr#L1-2"]);
    assert_eq!(record.related, ["dom-client-acme,inc"]);

    // Legacy documents that the strict page parser would refuse (duplicate
    // keys here) must still index, with the same comment/quote handling.
    let legacy = DocumentRecord::from_markdown(
        Path::new("sources/2026/10/src-20261028-legacy/source.md"),
        "---\n\
         id: src-20261028-legacy # source\n\
         title: First\n\
         title: \"Second # not a comment\" # comment\n\
         tags: [\"client:acme,inc\"]\n\
         ---\nLegacy body\n",
    );
    assert_eq!(legacy.id, "src-20261028-legacy");
    assert_eq!(legacy.title, "Second # not a comment");
    assert_eq!(legacy.tags, ["client:acme,inc"]);
}

#[test]
fn index_records_carry_the_template_and_the_document_date() {
    use crate::domain::knowledge::DocumentRecord;
    use std::path::Path;

    let record = |path: &str, text: &str| DocumentRecord::from_markdown(Path::new(path), text);
    let spec = record("knowledge/domain/dom-project-acme-renewal.md", SPEC_PAGE);
    assert_eq!(spec.template.as_deref(), Some("project"));
    assert_eq!(spec.date.as_deref(), Some("2026-10-28"));

    let source = record(
        "sources/2026/10/src-20261028-acme-qbr/source.md",
        "---\nid: src-20261028-acme-qbr\noccurred_at: 2026-10-27T23:00:00-04:00\ncaptured_at: 2026-10-28T15:02:11-04:00\n---\nx\n",
    );
    assert_eq!(
        source.date.as_deref(),
        Some("2026-10-27"),
        "occurred_at wins"
    );
    assert_eq!(source.template, None);

    let captured = record(
        "sources/2026/10/src-20261028-note/source.md",
        "---\nid: src-20261028-note\ncaptured_at: \"2026-10-29T08:00:00Z\"\n---\nx\n",
    );
    assert_eq!(captured.date.as_deref(), Some("2026-10-29"));

    let artifact = record(
        "artifacts/2026/10/art-20261029-report/artifact.md",
        "---\nid: art-20261029-report\nproduced_by: { crew: c, at: 2026-11-01T00:00:00Z }\n---\nx\n",
    );
    assert_eq!(
        artifact.date.as_deref(),
        Some("2026-10-29"),
        "the id's date"
    );

    let undated = record(
        "knowledge/domain/dom-acme.md",
        "---\nid: dom-acme\ntemplate: null\nupdated_at: soon\n---\nx\n",
    );
    assert_eq!(undated.date, None);
    assert_eq!(undated.template, None);
}
