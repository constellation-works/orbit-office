use crate::domain::knowledge::{DocumentKind, parse_citation, resolve_anchor};

#[test]
fn timestamp_overflow_is_refused_by_citation_resolution() {
    for stamp in ["1152921504606846976:00:00", "5124095576030431:00:16"] {
        assert!(resolve_anchor("[00:00:00] Jane: zero", &format!("t={stamp}")).is_err());
        assert!(resolve_anchor(&format!("[{stamp}] Jane: overflow"), "t=00:00:00").is_err());
    }
    for stamp in ["100:00:00", "5124095576040:25:51", "5124095576030431:00:15"] {
        assert!(resolve_anchor(&format!("[{stamp}] Jane: valid"), &format!("t={stamp}")).is_ok());
    }
}

#[test]
fn anchors_resolve_against_the_source_body() {
    let meeting = "[00:10:00] Jane: one\n\n[00:12:31] Dee: two\n";
    assert!(resolve_anchor(meeting, "t=00:12:31").is_ok());
    assert!(resolve_anchor(meeting, "t=00:11:00").is_ok());
    assert!(resolve_anchor(meeting, "t=00:09:59").is_err());
    assert!(resolve_anchor(meeting, "t=00:12:32").is_err());
    assert!(resolve_anchor("no turns", "t=00:00:00").is_err());
    assert!(resolve_anchor(meeting, "L1-3").is_ok());
    assert!(resolve_anchor(meeting, "L3").is_ok());
    assert!(resolve_anchor(meeting, "L4").is_err());
    assert!(resolve_anchor(meeting, "L0").is_err());
    assert!(resolve_anchor(meeting, "L3-1").is_err());
    let deck = "## Slide 1: Intro\nx\n\n## Slide 2\ny\n";
    assert!(resolve_anchor(deck, "slide=2").is_ok());
    assert!(resolve_anchor(deck, "slide=3").is_err());
    assert!(resolve_anchor("# Page 4\ntext\n", "page=4").is_ok());
    assert!(resolve_anchor(meeting, "frame=00:12:31").is_err());
    assert!(resolve_anchor(meeting, "nonsense").is_err());

    let cite = parse_citation("src-20261028-acme-qbr#t=00:12:31").expect("cite");
    assert_eq!(cite.kind, DocumentKind::Source);
    assert_eq!(cite.anchor.as_deref(), Some("t=00:12:31"));
    assert!(parse_citation("art-20261021-qbr-template").is_some());
    assert!(parse_citation("Jane Park").is_none());
    assert!(parse_citation("src-x#").is_none());
}
