//! Scribe v1 (specs/scribe.md): the parsed message file, the rules page
//! sections, the SCR file's name, front matter and entry lines, and the
//! model-free validation `office.scribe-append` applies. Pure: callers
//! read and write through the Library port.

mod append;
mod entries;
mod entry_line;
mod files;
mod messages;
mod naming;
mod paths;
mod rules;
mod time;

pub use append::append;
pub use entries::{Entry, Plan, Refusal, Request, Why, WhyNote, parse_entries};
pub use files::{Front, ScrFile, find_session, scr_files};
pub use messages::{
    Message, Parsed, from_jsonl, message_id, message_index, messages, parse_path, project_of,
    to_jsonl,
};
pub use naming::{display_title, first_words, free_id, humanized_title, planned, session_start};
pub use paths::{is_scr_path, scr_id, scr_path, slug, validate_scr_path, with_suffix};
pub use rules::{Authors, ScribeModels, ScribeRules, Tag, scribe_rules, topic_of};
pub use time::{line_time, local};

use crate::Error;

/// Library-relative directory of the parsed message files: inside the
/// Library's git-ignored `.office/state/` and its write grant.
pub const PARSE_DIR: &str = ".office/state/scribe";
/// Earlier messages returned as read-only context.
pub const CONTEXT_MESSAGES: usize = 10;
/// Longest title slug in an SCR file name.
pub const SLUG_MAX: usize = 48;
/// Session-id characters appended to a clashing SCR name.
pub const CLASH_SUFFIX: usize = 6;
/// Library directory of the SCR files.
pub const SCR_DIR: &str = "sources/conversation";
/// The one transcript format the conversation adapter reads.
pub const PROVIDER: &str = "claude-code";
/// Longest note text, in bytes.
pub const NOTE_MAX: usize = 4096;

/// Days after a preference is stated when it comes up for review.
pub const PREFERENCE_REVIEW_DAYS: i64 = 90;

fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}
