//! Page validation against a parsed template.

use crate::Error;
use crate::domain::knowledge::DocumentKind;
use crate::domain::template::{BUNDLED, Template};

use super::{PERSON, person};

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
