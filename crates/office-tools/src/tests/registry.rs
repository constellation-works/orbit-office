//! The registry: one row per tool name.

use std::collections::BTreeSet;

#[test]
fn tool_names_are_unique() {
    let mut seen = BTreeSet::new();
    for tool in crate::tools() {
        assert!(
            seen.insert(tool.spec().name),
            "duplicate tool {}",
            tool.spec().name
        );
    }
}
