use super::super::library_support::Env;
use super::{RENEWAL_PAGE, git, library_with_source};

#[test]
fn knowledge_write_refuses_unresolved_cites_without_writing_and_commits_a_valid_page_once() {
    let env = Env::new();
    let library = library_with_source(&env);
    let head = git(&library, &["rev-parse", "HEAD"]);
    let page = library.join("knowledge/domain/dom-project-acme-renewal.md");

    for (missing, cite) in [
        ("source", "src-20261028-missing#t=00:12:31"),
        ("anchor", "src-20261028-acme-qbr#t=01:00:00"),
        ("anchor", "src-20261028-acme-qbr#L99"),
    ] {
        let markdown = RENEWAL_PAGE.replace(
            "src-20261028-acme-qbr#t=00:12:31\nrefs",
            &format!("{cite}\nrefs"),
        );
        let refused = env.tool(
            "office.knowledge-write",
            serde_json::json!({ "markdown": markdown }),
        );
        assert_eq!(
            refused["ok"],
            serde_json::json!(false),
            "{missing}: {refused}"
        );
        assert_eq!(
            refused["error"]["code"],
            serde_json::json!("unresolved_cite"),
            "{missing}: {refused}"
        );
        assert!(!page.exists(), "a refused write leaves no page ({missing})");
        assert_eq!(
            git(&library, &["rev-parse", "HEAD"]),
            head,
            "no commit ({missing})"
        );
    }

    let written = env.tool(
        "office.knowledge-write",
        serde_json::json!({ "markdown": RENEWAL_PAGE }),
    );
    assert_eq!(written["ok"], serde_json::json!(true), "{written}");
    assert_eq!(
        written["output"],
        serde_json::json!({
            "id": "dom-project-acme-renewal",
            "type": "domain",
            "path": "knowledge/domain/dom-project-acme-renewal.md",
            "created": true,
        })
    );
    assert_eq!(std::fs::read_to_string(&page).unwrap(), RENEWAL_PAGE);
    let commits = git(
        &library,
        &["rev-list", "--count", &format!("{}..HEAD", head.trim())],
    );
    assert_eq!(commits.trim(), "1", "a valid write commits once");
    assert_eq!(
        git(&library, &["show", "--name-only", "--format=", "HEAD"]).trim(),
        "knowledge/domain/dom-project-acme-renewal.md"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["domain"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["library"]["pages_off_template"],
        serde_json::json!(0)
    );
}
