//! Template parsing and page validation through the public domain API.

use office_common::Sensitivity;

use crate::Error;
use crate::domain::knowledge::DocumentKind;
use crate::domain::template::{
    AttachPolicy, BUNDLED, KnowledgePage, Template, TemplateFile, TemplateKind, assess_templates,
};

const PERSON: &str = "---\ntemplate: person\ntype: domain\nid: dom-person-<slug>\ntitle: \"<Full name>\"\none_per_library: false\nattach: participants\nsensitivity: normal\nsections: [Role, Works with, What they care about, History]\nrequired: [Role]\n---\n## Role\nDescribe their work.\n";

fn person() -> Template {
    Template::from_markdown("person.md", PERSON).expect("valid person rules")
}

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

fn assert_invalid(text: &str, field: &str) {
    let error =
        Template::from_markdown("custom/person.md", text).expect_err("invalid template refused");
    assert_eq!(error.code(), "template_invalid");
    let Error::TemplateInvalid {
        file,
        field: actual,
        ..
    } = &error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(file, "custom/person.md");
    assert_eq!(actual, field);
    assert!(error.to_string().contains("custom/person.md"));
    assert!(error.to_string().contains(field));
    assert!(!error.retryable());
}

#[test]
fn unknown_rule_is_refused_with_file_and_field() {
    assert_invalid(
        &PERSON.replace("attach: participants", "attch: participants"),
        "attch",
    );
}

#[test]
fn every_missing_rule_is_refused_with_file_and_field() {
    for field in [
        "template",
        "type",
        "id",
        "title",
        "one_per_library",
        "attach",
        "sensitivity",
        "sections",
        "required",
    ] {
        let text = PERSON
            .lines()
            .filter(|line| !line.starts_with(&format!("{field}:")))
            .collect::<Vec<_>>()
            .join("\n");
        assert_invalid(&text, field);
    }
}

#[test]
fn invalid_attach_is_refused_with_file_and_field() {
    assert_invalid(
        &PERSON.replace("attach: participants", "attach: sometimes"),
        "attach",
    );
}

#[test]
fn malformed_scalar_diagnostic_names_its_rule() {
    assert_invalid(
        &PERSON.replace("attach: participants", "attach: \"participants"),
        "attach",
    );
}

#[test]
fn plain_apostrophes_and_quoted_escapes_are_preserved() {
    for (title, expected) in [
        ("The user's role", "The user's role"),
        ("'The user''s role'", "The user's role"),
        (
            "\"A \\\"quoted\\\", title #1\" # comment",
            "A \"quoted\", title #1",
        ),
    ] {
        let text = PERSON.replace("title: \"<Full name>\"", &format!("title: {title}"));
        assert_eq!(
            Template::from_markdown("person.md", &text).unwrap().title,
            expected
        );
    }
}

#[test]
fn malformed_and_inconsistent_rules_are_refused() {
    for (old, new, field) in [
        ("type: domain", "type: source", "type"),
        ("template: person", "template: ../person", "template"),
        ("id: dom-person-<slug>", "id: dom-<slug>-<slug>", "id"),
        ("id: dom-person-<slug>", "id: dom-person-*", "id"),
        ("one_per_library: false", "one_per_library: true", "id"),
        (
            "one_per_library: false",
            "one_per_library: \"false\"",
            "one_per_library",
        ),
        ("sensitivity: normal", "sensitivity: public", "sensitivity"),
        ("title: \"<Full name>\"", "title: \"\"", "title"),
        ("required: [Role]", "required: [Unknown]", "required"),
        ("required: [Role]", "required: [Role, Role]", "required"),
        ("required: [Role]", "required: [false]", "required"),
        ("required: [Role]", "required: Role", "required"),
        ("required: [Role]", "required: [Role", "required"),
        (
            "attach: participants",
            "attach: never\nattach: participants",
            "attach",
        ),
    ] {
        assert_invalid(&PERSON.replace(old, new), field);
    }
    assert_invalid("template: person", "frontmatter");
    assert_invalid("---\ntemplate: person\n---oops\n", "frontmatter");
}

#[test]
fn comments_quotes_block_lists_and_crlf_preserve_rules_and_body() {
    let text = PERSON
        .replace("type: domain", "type: domain # About page")
        .replace(
            "title: \"<Full name>\"",
            "title: 'Name, alias #1' # title comment",
        )
        .replace(
            "sections: [Role, Works with, What they care about, History]",
            "sections:\n  - Role\n  - 'Works, with'\n  - \"History #1\"",
        )
        .replace("required: [Role]", "required:\n  - Role")
        .replace('\n', "\r\n");
    let parsed = Template::from_markdown("person.md", &text).expect("supported YAML syntax");
    assert_eq!(parsed.title, "Name, alias #1");
    assert_eq!(parsed.sections, ["Role", "Works, with", "History #1"]);
    assert_eq!(parsed.required, ["Role"]);
    assert_eq!(parsed.body, "## Role\r\nDescribe their work.\r\n");
    assert_eq!(
        Template::from_markdown(
            "person.md",
            &PERSON.replace("sensitivity: normal", "sensitivity: private")
        )
        .unwrap()
        .sensitivity,
        Sensitivity::Private
    );
}

#[test]
fn wrong_page_id_is_refused() {
    for id in [
        "dom-client-jane",
        "dom-person-",
        "dom-person-Jane",
        "dom-person-../jane",
        "dom-person-jane_park",
        "dom-person-jane--park",
    ] {
        let error = person()
            .validate_page(id, DocumentKind::Domain, "## Role\nOwner")
            .expect_err("wrong id");
        assert_eq!(error.code(), "page_id_mismatch", "{id}");
    }
    let fixed = Template::from_markdown(
        "fixed.md",
        &PERSON
            .replace("dom-person-<slug>", "dom-person")
            .replace("one_per_library: false", "one_per_library: true"),
    )
    .unwrap();
    assert!(
        fixed
            .validate_page("dom-person", DocumentKind::Domain, "## Role\nOwner")
            .is_ok()
    );
    assert_eq!(
        fixed
            .validate_page("dom-person-jane", DocumentKind::Domain, "## Role\nOwner")
            .unwrap_err()
            .code(),
        "page_id_mismatch"
    );
}

#[test]
fn wrong_page_type_is_refused() {
    for kind in [
        DocumentKind::Runbook,
        DocumentKind::Source,
        DocumentKind::Artifact,
    ] {
        let error = person()
            .validate_page("dom-person-jane", kind, "## Role\nOwner")
            .expect_err("wrong type");
        assert_eq!(error.code(), "template_type_mismatch");
    }
}

#[test]
fn missing_or_empty_required_section_is_refused() {
    for body in [
        "",
        "## Works with\nOwner",
        "## Role\n \n## History\nOwner",
        "### Role\nOwner",
        "## role\nOwner",
        "```markdown\n## Role\nOwner\n```",
        "    ## Role\n    Owner",
        "## Role\n# Another page\nOwner",
        "## Role\n### Team\n",
    ] {
        let error = person()
            .validate_page("dom-person-jane", DocumentKind::Domain, body)
            .expect_err("missing or empty Role");
        assert_eq!(error.code(), "missing_section", "{body:?}");
        assert!(matches!(error, Error::MissingSection { section, .. } if section == "Role"));
    }
}

#[test]
fn a_page_meeting_its_template_is_accepted() {
    for body in [
        "## Role\nOwns renewal decisions.\n",
        "## Role ##\nOwns renewal decisions.",
        "  ## Role\n### Decisions\nOwns renewals.",
        "## Role\n\nOwns renewals.\n## History\n",
        "## Role\n\n## Role\nOwns renewals.",
    ] {
        person()
            .validate_page("dom-person-jane-park", DocumentKind::Domain, body)
            .expect("valid page");
    }
}

#[test]
fn a_required_section_can_have_fenced_code_as_its_content() {
    person()
        .validate_page(
            "dom-person-jane",
            DocumentKind::Domain,
            "## Role\n```text\nOwner\n```",
        )
        .expect("nonempty code is section content");
}

#[test]
fn indented_fence_markers_do_not_hide_real_sections() {
    person()
        .validate_page(
            "dom-person-jane",
            DocumentKind::Domain,
            "    ```text\n    An indented example\n\n## Role\nOwner\n",
        )
        .expect("indented code does not open a fenced block");
}

#[test]
fn html_comments_cannot_supply_required_headings_or_content() {
    for body in [
        "<!--\n## Role\nOwner\n-->",
        "## Role\n<!-- Owner -->\n",
        "## Role\n<!--\nOwner\n-->\n",
    ] {
        assert_eq!(
            person()
                .validate_page("dom-person-jane", DocumentKind::Domain, body)
                .unwrap_err()
                .code(),
            "missing_section"
        );
    }
}

#[test]
fn every_template_refusal_is_deterministic() {
    // Protocol refusals repeat the same answer on retry, so none is retryable.
    let refusals = [
        (
            person()
                .validate_page("dom-client-jane", DocumentKind::Domain, "## Role\nOwner")
                .expect_err("wrong id"),
            "page_id_mismatch",
        ),
        (
            person()
                .validate_page("dom-person-jane", DocumentKind::Runbook, "## Role\nOwner")
                .expect_err("wrong type"),
            "template_type_mismatch",
        ),
        (
            person()
                .validate_page("dom-person-jane", DocumentKind::Domain, "")
                .expect_err("missing section"),
            "missing_section",
        ),
        (
            Template::from_markdown(
                "custom/person.md",
                &PERSON.replace("attach: participants", "attach: sometimes"),
            )
            .expect_err("invalid rule"),
            "template_invalid",
        ),
    ];
    for (error, code) in refusals {
        assert_eq!(error.code(), code);
        assert!(!error.retryable(), "{code} must not be retryable");
    }
}

#[test]
fn runbooks_require_all_three_sections() {
    let bundled = BUNDLED
        .iter()
        .find(|entry| entry.filename == "runbook.md")
        .unwrap();
    let template = Template::from_markdown(bundled.filename, bundled.markdown).unwrap();
    for section in ["Inputs", "Steps", "Done when"] {
        let body = ["Inputs", "Steps", "Done when"]
            .into_iter()
            .filter(|title| *title != section)
            .map(|title| format!("## {title}\nContent\n"))
            .collect::<String>();
        let error = template
            .validate_page("rb-weekly-report", DocumentKind::Runbook, &body)
            .expect_err("each runbook section required");
        assert!(
            matches!(error, Error::MissingSection { section: actual, .. } if actual == section)
        );
    }
}

#[test]
fn assess_templates_counts_pages_and_names_broken_files() {
    let person = BUNDLED
        .iter()
        .find(|entry| entry.filename == "person.md")
        .expect("person template");
    let meets = "---\nid: dom-person-ada\ntype: domain\ntemplate: person\ntitle: Ada\n---\n\n## Role\nKeeps the notes.\n";
    let missing = "---\nid: dom-person-bev\ntype: domain\ntemplate: person\ntitle: Bev\n---\n\n## History\nNo role.\n";
    let free = "---\nid: dom-free\ntype: domain\ntitle: Free\n---\n\n## Role\nUnbound.\n";
    let unknown = "---\nid: dom-person-cy\ntype: domain\ntemplate: missing\ntitle: Cy\n---\n\n## Role\nPresent.\n";
    let report = assess_templates(
        &[
            TemplateFile {
                path: ".office/templates/person.md",
                markdown: Some(person.markdown),
            },
            TemplateFile {
                path: ".office/templates/notes.md",
                markdown: Some("not a template\n"),
            },
            TemplateFile {
                path: ".office/templates/linked.md",
                markdown: None,
            },
        ],
        &[
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: meets,
            },
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: missing,
            },
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: free,
            },
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: unknown,
            },
        ],
    );
    assert_eq!(report.pages_off_template, 2);
    assert_eq!(
        report.template_invalid,
        vec![
            ".office/templates/linked.md".to_string(),
            ".office/templates/notes.md".to_string(),
        ]
    );
}

#[test]
fn assess_templates_counts_commented_and_quoted_page_frontmatter() {
    let person = BUNDLED
        .iter()
        .find(|entry| entry.filename == "person.md")
        .expect("person template");
    let commented_template = r#"---
id: dom-person-ada
type: domain
template: person # selection
title: Ada
---

## Role
Keeps the notes.
"#;
    let commented_type = r#"---
id: dom-person-bev
type: domain # layer
template: person
title: Bev
---

## Role
Keeps the notes.
"#;
    let quoted = r#"---
id: dom-person-cy
type: "domain"
template: 'person'
title: Cy
---

## Role
Keeps the notes.
"#;
    let unknown = r#"---
id: dom-person-dee
type: domain
template: missing # intentional
title: Dee
---

## Role
Keeps the notes.
"#;
    let missing_section = r#"---
id: dom-person-eli
type: domain
template: person # current rules
title: Eli
---

## History
No role.
"#;

    let templates = [TemplateFile {
        path: ".office/templates/person.md",
        markdown: Some(person.markdown),
    }];
    let valid_report = assess_templates(
        &templates,
        &[
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: commented_template,
            },
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: commented_type,
            },
            KnowledgePage {
                kind: DocumentKind::Domain,
                markdown: quoted,
            },
        ],
    );
    let unknown_report = assess_templates(
        &templates,
        &[KnowledgePage {
            kind: DocumentKind::Domain,
            markdown: unknown,
        }],
    );
    let violation_report = assess_templates(
        &templates,
        &[KnowledgePage {
            kind: DocumentKind::Domain,
            markdown: missing_section,
        }],
    );

    assert_eq!(valid_report.pages_off_template, 0);
    assert_eq!(unknown_report.pages_off_template, 1);
    assert_eq!(violation_report.pages_off_template, 1);
}
