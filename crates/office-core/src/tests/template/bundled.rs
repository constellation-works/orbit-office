//! Bundled template rules and skeletons.

use office_common::Sensitivity;

use crate::domain::knowledge::DocumentKind;
use crate::domain::template::{AttachPolicy, BUNDLED, Template, TemplateKind};

#[test]
fn every_bundled_template_has_the_spec_rules_and_a_usable_skeleton() {
    // knowledge-templates.md: Bundled templates and the concrete sections table.
    let expected: &[(&str, &str, bool, AttachPolicy, &[&str])] = &[
        (
            "user",
            "dom-user",
            true,
            AttachPolicy::Always,
            &["Name", "Role"],
        ),
        (
            "standing-directions",
            "dom-standing-directions",
            true,
            AttachPolicy::Always,
            &["Directions"],
        ),
        (
            "systems",
            "dom-systems",
            true,
            AttachPolicy::Always,
            &["Tools"],
        ),
        (
            "person",
            "dom-person-<slug>",
            false,
            AttachPolicy::Participants,
            &["Role"],
        ),
        (
            "client",
            "dom-client-<slug>",
            false,
            AttachPolicy::Tagged,
            &["Overview"],
        ),
        (
            "project",
            "dom-project-<slug>",
            false,
            AttachPolicy::Tagged,
            &["Status", "Background"],
        ),
        (
            "topic",
            "dom-topic-<slug>",
            false,
            AttachPolicy::Tagged,
            &["Overview"],
        ),
        (
            "glossary",
            "dom-glossary",
            true,
            AttachPolicy::Matched,
            &["Terms"],
        ),
        (
            "goals",
            "dom-goals",
            true,
            AttachPolicy::Never,
            &["Priorities"],
        ),
        (
            "rhythm",
            "dom-rhythm",
            true,
            AttachPolicy::Never,
            &["Recurring meetings"],
        ),
        (
            "scribe-rules",
            "dom-scribe-rules",
            true,
            AttachPolicy::Never,
            &[],
        ),
        (
            "preferences",
            "dom-preferences-<slug>",
            false,
            AttachPolicy::Tagged,
            &[],
        ),
        (
            "runbook",
            "rb-<slug>",
            false,
            AttachPolicy::Never,
            &["Inputs", "Steps", "Done when"],
        ),
    ];
    assert_eq!(BUNDLED.len(), expected.len());
    for (bundled, &(name, pattern, one, attach, required)) in BUNDLED.iter().zip(expected) {
        let template = Template::from_markdown(bundled.filename, bundled.markdown)
            .expect("bundled rules parse");
        assert_eq!(bundled.filename, format!("{name}.md"));
        assert_eq!(template.template, name);
        assert_eq!(template.id.as_str(), pattern);
        assert_eq!(template.one_per_library, one);
        assert_eq!(template.attach, attach);
        assert_eq!(template.required, required);
        assert_eq!(template.sensitivity, Sensitivity::Normal);
        let (kind, document_kind) = if name == "runbook" {
            (TemplateKind::Runbook, DocumentKind::Runbook)
        } else {
            (TemplateKind::Domain, DocumentKind::Domain)
        };
        assert_eq!(template.kind, kind);
        let id = pattern.replace("<slug>", "example-name");
        template
            .validate_page(&id, document_kind, &template.body)
            .expect("required skeleton sections have content");
        let mut all_sections = template.clone();
        all_sections.required = all_sections.sections.clone();
        all_sections
            .validate_page(&id, document_kind, &template.body)
            .expect("every suggested section has a useful skeleton");
    }
}

#[test]
fn scribe_rules_template_declares_v1_sections_none_required_and_allows_old_pages() {
    let bundled = BUNDLED
        .iter()
        .find(|entry| entry.filename == "scribe-rules.md")
        .expect("scribe-rules template");
    let template = Template::from_markdown(bundled.filename, bundled.markdown)
        .expect("bundled scribe-rules parse");
    assert_eq!(
        template.sections,
        ["Tags", "Topics", "Matters", "Capture", "Models"]
    );
    assert!(template.required.is_empty());
    // An old live-scribe page keeps validating: no section is required,
    // so it is at most drift, never a missing-section refusal.
    template
        .validate_page(
            "dom-scribe-rules",
            DocumentKind::Domain,
            "## Note\nDecisions.\n\n## Skip\nStatus chatter.\n\n## Kinds\nBorderline guidance.\n",
        )
        .expect("old sections page still validates");
    template
        .validate_page("dom-scribe-rules", DocumentKind::Domain, "")
        .expect("empty page still validates");
}
