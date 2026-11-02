//! Validated preference apply boundary for the scribe append path.
//! Provenance is supplied by the model-free transcript adapter, never model output.

use office_core::domain::knowledge::{
    Page,
    preferences::{self, Conflict},
};
use office_core::ports::Library;
use office_core::{Error, Result};
use std::path::Path;

/// Provenance of a preference candidate after the adapter's trust checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// A literal marker in a trusted, typed user turn.
    UserMarker,
    /// A marker in the assistant's words.
    AgentMarker,
    /// A model inference, without a trusted literal user marker.
    Inferred,
    /// Provider-labelled pasted, quoted or harness content.
    Untrusted,
}

/// No write occurs for any `Propose` outcome. The caller files the proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The validated page was written once.
    Applied,
    /// Fall back to the reviewed proposal route, with a stable reason.
    Propose(&'static str),
}

/// Apply a complete page only for a trusted user marker in a small turn.
/// Semantic conflicts accompany model findings; deterministic conflicts are read
/// from the current page. Invalid pages/read failures are errors, never approval.
/// S4 supplies the trusted provenance and the complete originating turn.
pub fn apply_preference(
    library: &dyn Library,
    markdown: &str,
    provenance: Provenance,
    turn: &str,
    semantic_conflicts: &[Conflict],
) -> Result<ApplyOutcome> {
    if provenance != Provenance::UserMarker {
        return Ok(ApplyOutcome::Propose("untrusted_candidate"));
    }
    if turn.lines().count() > 40 || turn.chars().count() > 4_000 {
        return Ok(ApplyOutcome::Propose("paste_sized_turn"));
    }
    if !semantic_conflicts.is_empty() {
        return Ok(ApplyOutcome::Propose("conflict"));
    }
    let page = crate::tools::knowledge::write::validate(library, markdown)?;
    if page.template.as_deref() != Some("preferences") {
        return Err(Error::InvalidArgument(
            "apply requires a preferences page".into(),
        ));
    }
    let expected = if let Some(current) = library.get(&page.id)? {
        let text = library.read_text(Path::new(&current.path))?;
        let digest = office_core::domain::source::sha256_hex(text.as_bytes());
        let current = Page::from_markdown(&text)?;
        if !preferences::conflicts(&current, &page)?.is_empty() {
            return Ok(ApplyOutcome::Propose("conflict"));
        }
        Some(digest)
    } else {
        None
    };
    // Recheck the snapshot under the writer lock: a concurrent override or
    // first-page creation must not bypass the contradiction read above.
    match library.write_reviewed_page(Path::new(&page.path()), markdown, expected.as_deref()) {
        Ok(_) => Ok(ApplyOutcome::Applied),
        Err(Error::Library {
            code: "proposal_revision_changed",
            ..
        }) => Ok(ApplyOutcome::Propose("revision_changed")),
        Err(error) => Err(error),
    }
}
