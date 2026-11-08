//! Session, line, selection and request types.

use serde::{Deserialize, Serialize};

use super::recall;
use crate::Error;

/// Bounded raw transcript input (8 MiB); no raw transcript is persisted.
pub const INPUT_LIMIT: u64 = 8 * 1024 * 1024;
fn is_false(value: &bool) -> bool {
    !*value
}

/// One scrubbed, numbered text line; tool results and thinking are excluded.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    /// Zero-based position in the adapter output, used by model selections.
    pub line: usize,
    /// Transcript speaker, never caller-supplied authority.
    pub role: String,
    /// Original message timestamp.
    pub timestamp: String,
    /// Exact line after credential scrubbing and outer whitespace removal.
    pub text: String,
    /// Read-only context line in an incremental preview; never selectable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub context: bool,
    /// Inside a paste region the adapter can see (a `<pasted_content>`
    /// wrapper in a user record, the wrapper's own lines included). The
    /// text is kept, but a scribe marker on it is never one
    /// (the trust rule of the scribe, now `constellation-works/scribe`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pasted: bool,
}

/// Parsed Claude Code session; future providers implement this same shape.
#[derive(Clone, Debug, Serialize)]
pub struct Session {
    /// Provider inferred from the adapter's record format.
    pub orchestrator: String,
    /// Title from the transcript's title/summary record, if any.
    pub title: Option<String>,
    /// Latest `custom-title` record's `customTitle`, if any.
    pub custom_title: Option<String>,
    /// Last non-synthetic model reported by an assistant message.
    pub model: Option<String>,
    /// Working directory and branch of the first conversational record.
    pub project: Option<String>,
    /// Git branch at session start, absent outside a repository.
    pub branch: Option<String>,
    /// First supported conversational record timestamp.
    pub started_at: Option<String>,
    /// Latest supported conversational record timestamp.
    pub last_active_at: Option<String>,
    /// Checked against every conversational record's sessionId.
    pub session_id: String,
    /// Digest of the raw input bytes, used only for idempotency.
    pub transcript_sha256: String,
    /// True when any credential was removed.
    pub scrubbed: bool,
    /// Strictest configured, existing-source and credential-derived privacy.
    pub sensitivity: String,
    /// Text only, with original timestamps and roles.
    pub lines: Vec<Line>,
    /// SHA-256 of the raw transcript bytes up to each line's source record,
    /// parallel to `lines`. Lets a later pass prove the transcript only grew.
    #[serde(skip)]
    pub line_prefix_sha256: Vec<String>,
}
/// A cheap model selects evidence by number, never supplies replacement text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Zero-based line in the preview.
    pub line: usize,
    /// decision, direction, fact, action or open; no handoff or resolved.
    pub kind: String,
    /// Zero-based assistant question line a user reply answers. Lets a
    /// short reply become a `decision` only when the answer checks pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<usize>,
}
/// Transcript capture request, separate from upload and WebVTT capture.
pub struct Request {
    /// Contained UTF-8 JSONL input path.
    pub path: String,
    /// Expected transcript session id.
    pub session_id: String,
    /// Optional model evidence choices; absent uses conservative classification.
    pub selections: Option<Vec<Selection>>,
    /// Preview digest; required whenever selections are supplied.
    pub expected_sha256: Option<String>,
    /// First line this pass considers (zero-based, inclusive); earlier lines
    /// are read-only context and selecting one refuses the write.
    pub from_line: Option<usize>,
    /// Crew-written scribe handoff, present only when the rules page
    /// enables it; written by [`append_handoff`] or dropped.
    pub handoff: Option<String>,
    /// Hook context and crew/user title; never used as authority.
    pub context: recall::Context,
}
pub(crate) fn invalid(message: &'static str) -> Error {
    Error::Library {
        code: "invalid_session_transcript",
        message: message.into(),
    }
}
