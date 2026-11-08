use super::{code, page};
use crate::domain::knowledge::{DocumentKind, MAX_PAGE_WORDS, Performable, SubmissionVia};

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
