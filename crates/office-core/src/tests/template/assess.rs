//! Template-drift assessment of knowledge pages.

use crate::domain::knowledge::DocumentKind;
use crate::domain::template::{BUNDLED, KnowledgePage, TemplateFile, assess_templates};

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
