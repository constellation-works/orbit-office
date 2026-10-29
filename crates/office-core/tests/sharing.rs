//! Sharing contracts through the pure public API.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::domain::{
    knowledge::{DocumentRecord, Page},
    sharing::{self, SharingError},
};
use std::path::Path;

#[test]
fn sharing_field_is_strict_and_byte_preserving_with_both_line_endings() {
    for nl in ["\n", "\r\n"] {
        let text = format!(
            "---{nl}id: dom-person{nl}title: Person # keep{nl}shareable:  false  # approval{nl}refs:{nl}  - {{ label: Approval, url: https://example.invalid }}{nl}---{nl}Body with trailing spaces  {nl}"
        );
        let next = sharing::with_shareable(&text, true).unwrap();
        assert_eq!(next, text.replacen("false", "true", 1));
        assert_eq!(sharing::with_shareable(&next, true).unwrap(), next);
        assert_eq!(sharing::with_shareable(&next, false).unwrap(), text);
        assert!(
            DocumentRecord::from_markdown(Path::new("knowledge/domain/dom-person.md"), &next)
                .shareable
        );
        assert!(Page::from_markdown(&next).unwrap().shareable);
    }
    for value in ["null", "\"true\"", "1", "[]"] {
        let text = format!("---\nid: dom-person\ntitle: Person\nshareable: {value}\n---\nBody\n");
        assert_eq!(
            Page::from_markdown(&text).unwrap_err().code(),
            "invalid_frontmatter"
        );
    }
}

#[test]
fn approval_checks_frontmatter_inline_and_artifact_refs_once_per_own_id() {
    let text = "---\nid: dom-person\ntitle: Person\nshareable: true\ncites: [src-one#L1, dom-unshared, other/src-foreign#L1]\nrefs: [{label: Work, artifact: art-one}]\n---\nCitations [src-two#L1; art-one, src-one#L2].\n";
    let mut looked_up = Vec::new();
    let err = sharing::validate_sharing(text, |id| {
        looked_up.push(id.to_string());
        Ok(None)
    })
    .unwrap_err();
    let office_core::Error::Sharing(SharingError::CitesUnshared { ids }) = err else {
        panic!("{err}")
    };
    assert_eq!(ids, ["art-one", "dom-unshared", "src-one", "src-two"]);
    assert!(!looked_up.iter().any(|id| id.contains('/')));
    let err = sharing::validate_sharing(
        &text.replace("shareable: true", "sensitivity: private\nshareable: true"),
        |_| panic!("private check precedes lookup"),
    )
    .unwrap_err();
    assert_eq!(err.code(), "share_private_refused");
    assert!(err.to_string().contains("sensitivity"));
}

#[test]
fn shared_person_view_filters_refs_qualifies_cites_and_preserves_contributors() {
    let text = "---\nid: dom-person-jane\ntitle: Jane\ntemplate: person\nsensitivity: normal\nshareable: true\ncontributors: [colleague]\ncites: [src-one#L1, art-one, colleague/src-two#L2]\nrelated: [dom-approved, dom-shared, dom-personal]\nrefs:\n  - {label: Email, contact: jane@example.invalid}\n  - {label: File, path: /private/jane}\n  - {label: Private output, artifact: art-private}\n  - {label: Shared output, artifact: art-one}\n  - {label: Public, url: https://example.invalid}\n---\nJane owns this [src-one#L1; art-one, colleague/src-two#L2].\n";
    let next = sharing::shared_view(
        text,
        "jane",
        |id| ["art-one", "dom-approved"].contains(&id),
        |id| id == "dom-shared",
    )
    .unwrap();
    assert!(!next.contains("contact:") && !next.contains("path:"));
    assert!(!next.contains("art-private") && !next.contains("dom-personal"));
    assert!(!next.contains("sensitivity:") && !next.contains("shareable:"));
    assert!(next.contains("shared_by: \"jane\""));
    assert!(next.contains("contributors: [\"colleague\", \"jane\"]"));
    assert!(
        next.ends_with("Jane owns this [jane/src-one#L1; jane/art-one, colleague/src-two#L2].\n")
    );
    let doc =
        DocumentRecord::from_markdown(Path::new("knowledge/domain/dom-person-jane.md"), &next);
    assert_eq!(
        doc.cites,
        ["jane/src-one#L1", "jane/art-one", "colleague/src-two#L2"]
    );
    assert_eq!(doc.related, ["dom-approved", "dom-shared"]);
    let again = sharing::shared_view(&next, "jane", |_| true, |_| true).unwrap();
    assert_eq!(
        again.matches("\"jane\"").count(),
        2,
        "contributor is not duplicated"
    );
}

#[test]
fn shared_view_bytes_are_exact_for_nested_empty_escaped_unicode_null_and_bool_values() {
    let text = "---\nid: dom-flow\ntitle: Café\nempty_map: {}\nempty_list: []\nnested: { flag: true, inner: [\"a\\\"b\", \"🌍\"], missing: null }\nflag: false\n---\nBody.\n";
    let next = sharing::shared_view(text, "jane", |_| true, |_| true).unwrap();
    assert_eq!(
        next,
        "---\n\
         id: \"dom-flow\"\n\
         title: \"Café\"\n\
         empty_map: {  }\n\
         empty_list: []\n\
         nested: { flag: true, inner: [\"a\\\"b\", \"🌍\"], missing: null }\n\
         flag: false\n\
         contributors: [\"jane\"]\n\
         shared_by: \"jane\"\n\
         ---\nBody.\n"
    );
}

#[test]
fn knowledge_model_attachments_preserve_true_sharing_and_proposal_flags_require_booleans() {
    let markdown = "---\nid: dom-shared\ntitle: Shared\nshareable: true\n---\nBody.\n";
    let page = Page::from_markdown(markdown).unwrap();
    assert!(
        Page::from_markdown(&page.model_view().to_markdown())
            .unwrap()
            .shareable
    );
    for kind in ["knowledge_add", "knowledge_update"] {
        for value in [
            serde_json::json!(null),
            serde_json::json!("true"),
            serde_json::json!(1),
        ] {
            let mut payload =
                serde_json::json!({"kind":kind,"markdown":markdown,"shareable":value});
            if kind == "knowledge_update" {
                payload["base_sha256"] = serde_json::json!("fixture");
            }
            assert!(
                serde_json::from_value::<office_core::domain::proposal::Payload>(payload).is_err()
            );
        }
    }
}

#[test]
fn explicit_malformed_citations_are_refused_instead_of_escaping_approval() {
    let text = "---\nid: dom-malformed\ntitle: Malformed\nshareable: true\ncites: [src-unshared#bad anchor]\n---\nBody.\n";
    let error = sharing::validate_sharing(text, |_| {
        panic!("malformed citations must refuse before lookup")
    })
    .unwrap_err();
    assert_eq!(error.code(), "invalid_frontmatter");
}
