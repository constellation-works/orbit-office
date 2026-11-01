//! `gh` reply parsing for the PR source adapter.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_core::ports::{PrError, PrState};

use crate::pr::parse_view;

#[test]
fn parses_merged_open_and_invalid_replies() {
    let merged = parse_view(
        br#"{"state":"MERGED","mergedAt":"2026-10-28T10:00:00Z","mergeCommit":{"oid":"abc123"}}"#,
    )
    .unwrap();
    assert_eq!(merged.state, PrState::Merged);
    assert_eq!(merged.merged_at.as_deref(), Some("2026-10-28T10:00:00Z"));
    assert_eq!(merged.merge_commit.as_deref(), Some("abc123"));
    let open = parse_view(br#"{"state":"OPEN","mergedAt":null,"mergeCommit":null}"#).unwrap();
    assert_eq!(open.state, PrState::Open);
    assert_eq!(open.merged_at, None);
    for bad in [
        br#"{"state":"MERGED"}"#.as_slice(),
        br#"{"state":"WEIRD","mergedAt":null,"mergeCommit":null}"#.as_slice(),
        br#"not json"#.as_slice(),
        br#"{"state":"MERGED","mergedAt":42,"mergeCommit":null}"#.as_slice(),
    ] {
        assert_eq!(parse_view(bad), Err(PrError::InvalidResponse));
    }
}
