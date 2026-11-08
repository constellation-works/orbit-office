//! Template rule parsing refusals and supported syntax.

use office_common::Sensitivity;

use crate::domain::template::Template;

use super::{PERSON, assert_invalid};

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
