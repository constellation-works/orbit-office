//! Performable runbooks and submission rules.

use crate::domain::template::{Performable, SubmissionVia, Template};

use super::assert_invalid;

const RUNBOOK: &str = "---\ntemplate: runbook\ntype: runbook\nid: rb-<slug>\ntitle: \"<Task name>\"\none_per_library: false\nattach: never\nsensitivity: normal\nsections: [Inputs, Steps, Done when]\nrequired: [Inputs, Steps, Done when]\n---\n## Inputs\nEvidence.\n\n## Steps\nSteps.\n\n## Done when\nDone.\n";

#[test]
fn performable_agent_with_submission_draft_is_accepted() {
    let text = RUNBOOK.replace(
        "required: [Inputs, Steps, Done when]",
        "required: [Inputs, Steps, Done when]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: draft",
    );
    let template = Template::from_markdown("runbook.md", &text).expect("performable runbook");
    assert_eq!(template.performable, Performable::Agent);
    assert_eq!(template.performable.as_str(), "agent");
    let submission = template.submission.expect("submission block");
    assert_eq!(submission.system, "ServiceNow");
    assert_eq!(submission.via, SubmissionVia::Draft);
    assert_eq!(submission.via.as_str(), "draft");
    assert_eq!(submission.connection, None);
}

#[test]
fn performable_defaults_to_you_without_submission() {
    let template = Template::from_markdown("runbook.md", RUNBOOK).expect("plain runbook");
    assert_eq!(template.performable, Performable::You);
    assert_eq!(template.submission, None);
}

#[test]
fn submission_gateway_with_connection_is_accepted() {
    let text = RUNBOOK.replace(
        "required: [Inputs, Steps, Done when]",
        "required: [Inputs, Steps, Done when]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: gateway\n  connection: servicenow",
    );
    let template = Template::from_markdown("runbook.md", &text).expect("gateway runbook");
    let submission = template.submission.expect("submission block");
    assert_eq!(submission.via, SubmissionVia::Gateway);
    assert_eq!(submission.connection.as_deref(), Some("servicenow"));
}

#[test]
fn unknown_via_is_refused_naming_via() {
    let text = RUNBOOK.replace(
        "required: [Inputs, Steps, Done when]",
        "required: [Inputs, Steps, Done when]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: portal",
    );
    assert_invalid(&text, "via");
}

#[test]
fn gateway_without_connection_is_refused_naming_connection() {
    let text = RUNBOOK.replace(
        "required: [Inputs, Steps, Done when]",
        "required: [Inputs, Steps, Done when]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: gateway",
    );
    assert_invalid(&text, "connection");
}

#[test]
fn unknown_performable_is_refused_naming_performable() {
    let text = RUNBOOK.replace(
        "required: [Inputs, Steps, Done when]",
        "required: [Inputs, Steps, Done when]\nperformable: anyone",
    );
    assert_invalid(&text, "performable");
}
