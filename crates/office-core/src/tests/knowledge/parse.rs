use super::{SPEC_PAGE, code, page};
use crate::Error;
use crate::domain::knowledge::{DocumentKind, Page, PageStatus, RefTarget};
use office_common::Sensitivity;

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
fn scribe_settings_parse_only_on_the_scribe_rules_page() {
    let on = page(
        "---\nid: dom-scribe-rules\ntemplate: scribe-rules\ntitle: Scribe rules\nscribe:\n  handoff: true\n---\n## Note\nx\n",
    );
    assert_eq!(on.scribe_handoff, Some(true));
    on.check_base_rules()
        .expect("scribe settings on the rules page");
    let absent = page("---\nid: dom-scribe-rules\ntitle: Scribe rules\n---\n");
    assert_eq!(absent.scribe_handoff, None);
    let elsewhere = "---\nid: dom-user\ntitle: Me\nscribe: { handoff: false }\n---\n";
    assert_eq!(
        code(page(elsewhere).check_base_rules()),
        "invalid_frontmatter"
    );
    for value in [
        "scribe: true",
        "scribe: { handoff: yes }",
        "scribe: { notes: true }",
    ] {
        let markdown = format!("---\nid: dom-scribe-rules\ntitle: Scribe rules\n{value}\n---\n");
        assert_eq!(
            code(Page::from_markdown(&markdown)),
            "invalid_frontmatter",
            "{value}"
        );
    }
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
