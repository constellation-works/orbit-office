use super::SPEC_PAGE;

#[test]
fn index_records_decode_comments_quoted_scalars_and_quoted_list_items() {
    use crate::domain::knowledge::DocumentRecord;
    use std::path::Path;

    let record = DocumentRecord::from_markdown(
        Path::new("knowledge/domain/dom-person-ada.md"),
        "---\n\
         id: dom-person-ada # identity\n\
         type: domain # domain | runbook\n\
         title: \"Ada \\\"Ada\\\" Lovelace\" # name\n\
         summary: 'It''s Ada' # summary\n\
         sensitivity: private # privacy\n\
         tags: [\"client:acme,inc\", plain] # tags\n\
         cites: [\"src-20261028-acme-qbr#L1-2\"]\n\
         related:\n\
         \x20 - \"dom-client-acme,inc\" # related\n\
         ---\nBody\n",
    );
    assert_eq!(record.id, "dom-person-ada");
    assert_eq!(record.title, "Ada \"Ada\" Lovelace");
    assert_eq!(record.summary, "It's Ada");
    assert_eq!(record.sensitivity, "private");
    assert_eq!(record.tags, ["client:acme,inc", "plain"]);
    assert_eq!(record.cites, ["src-20261028-acme-qbr#L1-2"]);
    assert_eq!(record.related, ["dom-client-acme,inc"]);

    // Legacy documents that the strict page parser would refuse (duplicate
    // keys here) must still index, with the same comment/quote handling.
    let legacy = DocumentRecord::from_markdown(
        Path::new("sources/2026/10/src-20261028-legacy/source.md"),
        "---\n\
         id: src-20261028-legacy # source\n\
         title: First\n\
         title: \"Second # not a comment\" # comment\n\
         tags: [\"client:acme,inc\"]\n\
         ---\nLegacy body\n",
    );
    assert_eq!(legacy.id, "src-20261028-legacy");
    assert_eq!(legacy.title, "Second # not a comment");
    assert_eq!(legacy.tags, ["client:acme,inc"]);
}

#[test]
fn index_records_carry_the_template_and_the_document_date() {
    use crate::domain::knowledge::DocumentRecord;
    use std::path::Path;

    let record = |path: &str, text: &str| DocumentRecord::from_markdown(Path::new(path), text);
    let spec = record("knowledge/domain/dom-project-acme-renewal.md", SPEC_PAGE);
    assert_eq!(spec.template.as_deref(), Some("project"));
    assert_eq!(spec.date.as_deref(), Some("2026-10-28"));

    let source = record(
        "sources/2026/10/src-20261028-acme-qbr/source.md",
        "---\nid: src-20261028-acme-qbr\noccurred_at: 2026-10-27T23:00:00-04:00\ncaptured_at: 2026-10-28T15:02:11-04:00\n---\nx\n",
    );
    assert_eq!(
        source.date.as_deref(),
        Some("2026-10-27"),
        "occurred_at wins"
    );
    assert_eq!(source.template, None);

    let captured = record(
        "sources/2026/10/src-20261028-note/source.md",
        "---\nid: src-20261028-note\ncaptured_at: \"2026-10-29T08:00:00Z\"\n---\nx\n",
    );
    assert_eq!(captured.date.as_deref(), Some("2026-10-29"));

    let artifact = record(
        "artifacts/2026/10/art-20261029-report/artifact.md",
        "---\nid: art-20261029-report\nproduced_by: { crew: c, at: 2026-11-01T00:00:00Z }\n---\nx\n",
    );
    assert_eq!(
        artifact.date.as_deref(),
        Some("2026-10-29"),
        "the id's date"
    );

    let undated = record(
        "knowledge/domain/dom-acme.md",
        "---\nid: dom-acme\ntemplate: null\nupdated_at: soon\n---\nx\n",
    );
    assert_eq!(undated.date, None);
    assert_eq!(undated.template, None);
}
