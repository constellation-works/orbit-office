use super::super::library_support::Env;
use super::{RENEWAL_PAGE, library_with_source};

#[test]
fn knowledge_show_returns_the_model_view_and_the_sources_and_pages_linking_to_it() {
    let env = Env::new();
    let _library = library_with_source(&env);
    let written = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": RENEWAL_PAGE }),
    );
    assert_eq!(written["ok"], serde_json::json!(true), "{written}");
    let prep = "---\nid: rb-client-qbr-prep\ntitle: QBR prep\ncites: [dom-project-acme-renewal]\nrelated: [dom-project-acme-renewal]\nperformable: agent\nsubmission:\n  system: ServiceNow\n  via: draft\n---\n## Inputs\nThe renewal page.\n";
    let runbook = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": prep }),
    );
    assert_eq!(runbook["ok"], serde_json::json!(true), "{runbook}");
    assert_eq!(
        runbook["output"]["path"],
        serde_json::json!("knowledge/runbooks/rb-client-qbr-prep.md")
    );

    let shown = env.tool(
        "office.knowledge-show",
        serde_json::json!({ "id": "dom-project-acme-renewal" }),
    );
    assert_eq!(shown["ok"], serde_json::json!(true), "{shown}");
    let output = &shown["output"];
    assert_eq!(
        output["backlinks"],
        serde_json::json!([
            {
                "id": "rb-client-qbr-prep",
                "kind": "runbook",
                "title": "QBR prep",
                "path": "knowledge/runbooks/rb-client-qbr-prep.md",
                "via": ["cites", "related"],
            },
            {
                "id": "src-20261028-acme-qbr",
                "kind": "source",
                "title": "Acme QBR",
                "path": "sources/2026/10/src-20261028-acme-qbr/source.md",
                "via": ["related"],
            },
        ])
    );
    assert_eq!(output["page"]["template"], serde_json::json!("project"));
    assert_eq!(
        output["page"]["refs"],
        serde_json::json!([
            {"label": "Renewal tickets", "target": "account", "value": "jira:ACME", "authoritative": false, "as_of": null},
            {"label": "Jane Park", "target": "contact", "value": null, "authoritative": false, "as_of": null},
        ])
    );
    assert!(
        !shown.to_string().contains("jane@acme.example"),
        "the contact value never leaves through show: {shown}"
    );

    let missing = env.tool(
        "office.knowledge-show",
        serde_json::json!({ "id": "dom-absent" }),
    );
    assert_eq!(
        missing["error"]["code"],
        serde_json::json!("page_not_found")
    );
}
