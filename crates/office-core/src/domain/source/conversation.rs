//! Session transcript adapters, evidence selection and source provenance.

/// Append-only Scribe corrections and user-attributed entries.
pub mod corrections;
/// Stored reference extraction and model-free recall selection.
pub mod recall;
/// Scribe v1: parsed messages, rules sections and SCR files.
pub mod scribe;

mod cues;
mod handoff;
mod parse;
mod paste;
mod provenance;
mod scrub;
mod types;

pub use cues::{entries, note_kind};
pub use handoff::{
    Cursor, HandoffOutcome, RULES_PAGE_PATH, Rules, append_handoff, cursor, find_rules, rules,
    with_handoff,
};
pub use parse::parse;
pub use paste::{brief_first_sentence, brief_task_title, is_side_session_brief};
pub use provenance::{extracted_state, merge, prefix_grew, scrub_source, validate_provenance};
pub use scrub::scrub;
pub(crate) use types::invalid;
pub use types::{INPUT_LIMIT, Line, Request, Selection, Session};
