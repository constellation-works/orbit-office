//! Session scope rules: the current session context and global handoffs.
use crate::Result;
use crate::domain::metrics::timestamp;

use super::helpers::invalid;

/// The current session's thread identity, when the caller knows it. Both
/// fields are optional: a brief without a project keeps the legacy
/// single-conversation behaviour exactly.
#[derive(Clone, Debug, Default)]
pub struct SessionContext {
    /// Current project (a repository name or a working-directory path; it is
    /// normalized to the repository name before comparison).
    pub project: Option<String>,
    /// Current orchestrator (for example `claude` or `codex`), used to lead
    /// with the current orchestrator's thread inside the current project.
    /// It is the provider name conversation sources record, not a crew name.
    pub orchestrator: Option<String>,
    /// The current instant (RFC 3339) from the Clock port; a global handoff
    /// whose `until` is at or before it has expired. `None` never expires.
    pub now: Option<String>,
    /// Manual project choice (`brief.handoff: manual`, or `auto` with no
    /// single project): no project handoff and no `other_sessions`; the
    /// `projects` section lists the projects that have a thread instead.
    pub manual: bool,
}

/// An explicit `scope: global` declaration on a handoff entry
/// (specs/sessions.md#scopes). Set by the writer in `office.capture`, never
/// inferred: the stored marker is refused on a handoff without the scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalScope {
    /// Optional RFC 3339 expiry; the handoff leaves briefs once it passes.
    pub until: Option<String>,
}

impl GlobalScope {
    /// The marker leading the stored entry text: `(global)` or
    /// `(global until <RFC 3339>)`.
    #[must_use]
    pub fn marker(&self) -> String {
        match &self.until {
            Some(until) => format!("(global until {until})"),
            None => "(global)".into(),
        }
    }
}

/// Whether stored handoff text starts with a global marker, well-formed or
/// not. Capture refuses such text on a handoff that did not declare the
/// scope, so the marker cannot be forged by ordinary entry text.
#[must_use]
pub fn looks_global(text: &str) -> bool {
    text.starts_with("(global)") || text.starts_with("(global until ")
}

/// The scope a stored handoff text declares: `Ok(None)` for a project
/// handoff, the scope for `(global) …` or `(global until <time>) …`, and an
/// error for a malformed marker.
pub fn stored_scope(text: &str) -> Result<Option<GlobalScope>> {
    if !looks_global(text) {
        return Ok(None);
    }
    if let Some(rest) = text.strip_prefix("(global)") {
        if rest.starts_with(' ') && !rest.trim().is_empty() {
            return Ok(Some(GlobalScope::default()));
        }
        return Err(invalid("global handoff marker"));
    }
    let rest = text
        .strip_prefix("(global until ")
        .ok_or_else(|| invalid("global handoff marker"))?;
    let (until, tail) = rest
        .split_once(')')
        .ok_or_else(|| invalid("global handoff marker"))?;
    if timestamp(until).is_none() || !tail.starts_with(' ') || tail.trim().is_empty() {
        return Err(invalid("global handoff until"));
    }
    Ok(Some(GlobalScope {
        until: Some(until.into()),
    }))
}

/// `other_sessions`-style cap for the `global` section: at most this many
/// open global handoffs are listed, newest first.
pub const GLOBAL_HANDOFFS_LIMIT: usize = 5;
/// Cap for the `projects` section (projects with a handoff thread).
pub const PROJECTS_LIMIT: usize = 20;

/// `other_sessions` window: only handoffs at most this many seconds older
/// than the scoped `since` reference are listed.
pub const OTHER_SESSIONS_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;
/// `other_sessions` cap: at most this many other threads are listed.
pub const OTHER_SESSIONS_LIMIT: usize = 5;
