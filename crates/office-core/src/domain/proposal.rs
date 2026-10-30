//! Proposal payloads, evidence identity and thirty-day dismissal memory.

use super::{metrics::timestamp, source::sha256_hex};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Kinds of user-reviewed changes. Recurring work is always an auto-task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    /// Create and enable a recurring task.
    AutoTask,
    /// Add a knowledge page.
    KnowledgeAdd,
    /// Replace a page after checking its reviewed revision.
    KnowledgeUpdate,
    /// Write a runbook from recurring friction.
    RunbookFromFriction,
    /// Disable an auto-task or retire a page.
    Retire,
    /// Resolve a page changed both here and in the shared library.
    SharedConflict,
}
impl ProposalKind {
    /// Stable task-tag token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoTask => "auto_task",
            Self::KnowledgeAdd => "knowledge_add",
            Self::KnowledgeUpdate => "knowledge_update",
            Self::RunbookFromFriction => "runbook_from_friction",
            Self::Retire => "retire",
            Self::SharedConflict => "shared_conflict",
        }
    }
}

/// Closed task-minting definition. Steps are pinned instructions for the
/// minted task, not scheduler fields; acceptance prepares every script pin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoTask {
    /// Unique Orbit definition name.
    pub name: String,
    /// What the automation does.
    #[serde(default)]
    pub description: String,
    /// Exactly one cron or interval trigger.
    pub schedule: Schedule,
    /// Orbit's closed task template (validated below before any write).
    pub template: Map<String, Value>,
    /// Open-task deduplication policy.
    #[serde(default = "default_dedupe")]
    pub dedupe: String,
    /// Concrete, pinned plugin tool calls included in the task instructions.
    #[serde(default)]
    pub steps: Vec<Step>,
}
fn default_dedupe() -> String {
    "skip_if_open".into()
}

/// Trigger supported by Office proposals.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Schedule {
    /// Five-field cron expression.
    Cron {
        /// Cron expression.
        cron: String,
    },
    /// Positive interval in minutes.
    Interval {
        /// Interval.
        every_minutes: u32,
    },
}

/// One deterministic plugin tool call; no executable, identity or secrets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Always `plugin.tool_call`.
    pub kind: String,
    /// Canonical tool name.
    pub tool: String,
    /// Complete pinned tool input.
    pub input: Map<String, Value>,
}

/// A draft starts from the named template's current skeleton. The create
/// handler materializes it to Markdown before filing the reviewed artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageDraft {
    /// Sharing approval, checked at creation and again at acceptance.
    #[serde(default)]
    pub shareable: bool,
    /// Canonical domain or runbook id.
    pub id: String,
    /// Page title.
    pub title: String,
    /// Optional template name.
    #[serde(default)]
    pub template: Option<String>,
    /// Free-form content for an untemplated page.
    #[serde(default)]
    pub body: String,
    /// Replacement contents by level-two heading for a templated page.
    #[serde(default)]
    pub sections: BTreeMap<String, String>,
    /// Resolved Library citations, including source anchors.
    #[serde(default)]
    pub cites: Vec<String>,
}

/// Approved changes stored as `proposal.json`. There is no routine variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    /// Add and enable an Orbit auto-task.
    AutoTask {
        /// Concrete definition.
        definition: AutoTask,
    },
    /// Create a page.
    KnowledgeAdd {
        /// Optional sharing approval applied to the reviewed page.
        #[serde(
            default,
            deserialize_with = "sharing_flag",
            skip_serializing_if = "Option::is_none"
        )]
        shareable: Option<bool>,
        /// Draft to materialize at creation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<PageDraft>,
        /// Materialized reviewed Markdown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        markdown: Option<String>,
    },
    /// Replace exactly the reviewed page revision.
    KnowledgeUpdate {
        /// Optional sharing approval applied to the reviewed page.
        #[serde(
            default,
            deserialize_with = "sharing_flag",
            skip_serializing_if = "Option::is_none"
        )]
        shareable: Option<bool>,
        /// Complete replacement.
        markdown: String,
        /// SHA-256 of the current Markdown.
        base_sha256: String,
    },
    /// Create a runbook.
    RunbookFromFriction {
        /// Draft to materialize at creation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<PageDraft>,
        /// Materialized reviewed Markdown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        markdown: Option<String>,
    },
    /// Disable or retire.
    Retire {
        /// Exact reviewed target.
        target: RetireTarget,
    },
    /// A page changed both in the user's Library and in the shared library.
    /// Filed only by `office.library-sync`; accepted with a `choice`.
    SharedConflict {
        /// The page id.
        page: String,
        /// The page's clone-relative path in the shared library.
        path: String,
        /// The shared blob this conflict is about.
        blob: String,
        /// Their version, localized as the user's own copy (**Use theirs**).
        markdown: String,
        /// SHA-256 of the user's copy when the conflict was filed.
        base_sha256: String,
    },
}

fn sharing_flag<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}

/// Targets retirement can affect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetireTarget {
    /// Disable the named auto-task.
    AutoTask {
        /// Definition name.
        name: String,
    },
    /// Mark the reviewed page revision retired.
    Page {
        /// Library id.
        id: String,
        /// SHA-256 of the current Markdown.
        base_sha256: String,
    },
}
impl Payload {
    /// Apply an explicit proposal approval to its materialized Markdown.
    /// This pure normalization runs at creation and again after digest validation
    /// at acceptance, so direct reviewed payloads honor the same field.
    pub fn apply_sharing(&mut self) -> crate::Result<()> {
        match self {
            Self::KnowledgeAdd {
                markdown: Some(markdown),
                shareable: Some(value),
                ..
            }
            | Self::KnowledgeUpdate {
                markdown,
                shareable: Some(value),
                ..
            } => {
                *markdown = crate::domain::sharing::with_shareable(markdown, *value)?;
            }
            Self::KnowledgeAdd { .. }
            | Self::KnowledgeUpdate { .. }
            | Self::AutoTask { .. }
            | Self::RunbookFromFriction { .. }
            | Self::Retire { .. }
            | Self::SharedConflict { .. } => {}
        }
        Ok(())
    }

    /// Stable kind independent of payload shape.
    #[must_use]
    pub fn kind(&self) -> ProposalKind {
        match self {
            Self::AutoTask { .. } => ProposalKind::AutoTask,
            Self::KnowledgeAdd { .. } => ProposalKind::KnowledgeAdd,
            Self::KnowledgeUpdate { .. } => ProposalKind::KnowledgeUpdate,
            Self::RunbookFromFriction { .. } => ProposalKind::RunbookFromFriction,
            Self::Retire { .. } => ProposalKind::Retire,
            Self::SharedConflict { .. } => ProposalKind::SharedConflict,
        }
    }
}

/// Closed input to proposal creation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    /// User-visible title.
    pub title: String,
    /// What accepting changes.
    pub description: String,
    /// Task, friction, page or anchored source references.
    pub evidence: Vec<String>,
    /// Concrete reviewed payload.
    pub payload: Payload,
    /// Additional task tags, including `onboarding`.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Safe field-specific refusal, with no payload echoed into errors.
#[must_use]
pub fn invalid(field: &str) -> Error {
    Error::InvalidArgument(format!("proposal requires valid {field}"))
}

/// Refusal after checking the persisted proposal lifecycle.
#[must_use]
pub fn refused(code: &'static str, message: &str) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

/// A canonical kind plus evidence-set identity, independent of order/duplicates.
pub fn cluster(kind: ProposalKind, evidence: &[String]) -> Result<String> {
    if evidence.is_empty()
        || evidence
            .iter()
            .any(|s| s.trim().is_empty() || s.contains(['\n', '\r']))
    {
        return Err(invalid("nonempty evidence references"));
    }
    let mut sorted = evidence.to_vec();
    sorted.sort();
    sorted.dedup();
    let bytes = serde_json::to_vec(&(kind, sorted)).map_err(|_| invalid("evidence"))?;
    Ok(format!("proposal-cluster:{}", sha256_hex(&bytes)))
}

/// Rejected proposals suppress equivalent evidence for thirty elapsed days.
/// Incomplete dismissal evidence refuses the read instead of re-proposing.
pub fn recently_dismissed(task: &Value, tag: &str, now: i64) -> Result<bool> {
    if task.get("status").and_then(Value::as_str) != Some("rejected")
        || !task
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.iter().any(|v| v.as_str() == Some(tag)))
    {
        return Ok(false);
    }
    let at = task
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(timestamp)
        .ok_or_else(|| invalid("dismissal updated_at"))?;
    Ok(at <= now && now - at < 30 * 86400)
}

impl AutoTask {
    /// Validate the closed callback surface before reserving a proposal.
    pub fn validate(&self) -> Result<()> {
        if !valid_name(&self.name) || !["skip_if_open", "always"].contains(&self.dedupe.as_str()) {
            return Err(invalid("auto-task name and dedupe"));
        }
        match &self.schedule {
            Schedule::Cron { cron } if valid_cron(cron) => {}
            Schedule::Interval { every_minutes } if *every_minutes > 0 => {}
            Schedule::Cron { .. } | Schedule::Interval { .. } => return Err(invalid("schedule")),
        }
        let allowed = [
            "title",
            "description",
            "acceptance_criteria",
            "task_type",
            "tags",
            "required_tools",
            "priority",
            "complexity",
            "crew",
            "status",
        ];
        if self.template.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(invalid("closed auto-task template"));
        }
        for key in ["title", "description", "crew"] {
            if let Some(value) = self.template.get(key)
                && value.as_str().is_none_or(|v| v.trim().is_empty())
            {
                return Err(invalid("template text"));
            }
        }
        if !self.template.contains_key("title") {
            return Err(invalid("template.title"));
        }
        for key in ["acceptance_criteria", "tags", "required_tools"] {
            if let Some(value) = self.template.get(key)
                && !value.as_array().is_some_and(|values| {
                    values
                        .iter()
                        .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
                })
            {
                return Err(invalid("template lists"));
            }
        }
        for (key, values) in [
            ("task_type", &["feature", "bug", "refactor", "chore"][..]),
            ("priority", &["low", "medium", "high", "critical"][..]),
            ("complexity", &["low", "medium", "hard", "xhard"][..]),
            ("status", &["proposed", "backlog"][..]),
        ] {
            if let Some(value) = self.template.get(key)
                && !value.as_str().is_some_and(|v| values.contains(&v))
            {
                return Err(invalid("template enums"));
            }
        }
        for step in &self.steps {
            if step.kind != "plugin.tool_call"
                || !step.tool.contains('.')
                || step.tool.contains(char::is_whitespace)
            {
                return Err(invalid("plugin.tool_call step"));
            }
        }
        Ok(())
    }

    /// Only Orbit-supported keys are forwarded. Pin steps into the minted
    /// task's description and required tools, rather than inventing a scheduler
    /// `steps` field that Orbit would refuse.
    pub fn callback(&self) -> Result<Map<String, Value>> {
        self.validate()?;
        let mut template = self.template.clone();
        if !self.steps.is_empty() {
            let steps = serde_json::to_string_pretty(&self.steps).map_err(|_| invalid("steps"))?;
            let description = template
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            template.insert(
                "description".into(),
                Value::String(format!(
                    "{description}\n\nApproved tool calls (in order):\n```json\n{steps}\n```"
                )),
            );
            let mut required = template
                .get("required_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for step in &self.steps {
                let tool = Value::String(step.tool.clone());
                if !required.contains(&tool) {
                    required.push(tool);
                }
            }
            template.insert("required_tools".into(), Value::Array(required));
        }
        serde_json::json!({"name":self.name,"description":self.description,"schedule":self.schedule,"template":template,"dedupe":self.dedupe}).as_object().cloned().ok_or_else(|| invalid("definition"))
    }
}

/// Definition-name grammar, also used for safe evidence-page filenames.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Where a proposal's evidence page sits, by the app's words for its status.
/// The record is the Orbit task; the page only follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProposalStatus {
    /// Still waiting for the user (Orbit `proposed`).
    Open,
    /// Accepted (Orbit `done`).
    Accepted,
    /// Dismissed (Orbit `rejected`).
    Dismissed,
}
impl ProposalStatus {
    /// The folder under `proposals/`.
    #[must_use]
    pub fn folder(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Accepted => "accepted",
            Self::Dismissed => "dismissed",
        }
    }

    /// The folder an Orbit task status files its page under. Any other
    /// status (an acceptance in progress or review, a backlog or archived
    /// task) has no folder of its own, so its page is not moved.
    #[must_use]
    pub fn from_orbit(status: &str) -> Option<Self> {
        match status {
            "proposed" => Some(Self::Open),
            "done" => Some(Self::Accepted),
            "rejected" => Some(Self::Dismissed),
            _ => None,
        }
    }

    fn from_folder(folder: &str) -> Option<Self> {
        match folder {
            "open" => Some(Self::Open),
            "accepted" => Some(Self::Accepted),
            "dismissed" => Some(Self::Dismissed),
            _ => None,
        }
    }
}

/// A proposal page's place in the Library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordLocation {
    /// Legacy flat `proposals/<id>.md`, read until reconciled.
    Flat,
    /// `proposals/open/<id>.md`.
    Open,
    /// `proposals/{accepted,dismissed}/YYYY/MM/<id>.md`, dated by closing.
    Closed {
        /// Accepted or dismissed.
        status: ProposalStatus,
        /// Four-digit closing year.
        year: String,
        /// Two-digit closing month.
        month: String,
    },
}
impl RecordLocation {
    /// The status folder this page sits in, `None` for a legacy flat page.
    #[must_use]
    pub fn status(&self) -> Option<ProposalStatus> {
        match self {
            Self::Flat => None,
            Self::Open => Some(ProposalStatus::Open),
            Self::Closed { status, .. } => Some(*status),
        }
    }
}

/// A validated proposal page path: the proposal id (the file stem) and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordPath {
    /// File stem; the Orbit task id for every page Office writes.
    pub id: String,
    /// Status folder or the legacy flat layout.
    pub location: RecordLocation,
}

/// Parse a Library-relative proposal page path. Accepted layouts are
/// `proposals/open/<id>.md`, `proposals/{accepted,dismissed}/YYYY/MM/<id>.md`
/// and the legacy flat `proposals/<id>.md`; anything else is refused.
pub fn parse_record_path(relative: &std::path::Path) -> Result<RecordPath> {
    let mut parts = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(invalid("proposals/<status>/<id>.md path"));
        };
        parts.push(
            name.to_str()
                .ok_or_else(|| invalid("proposals/<status>/<id>.md path"))?,
        );
    }
    let digits =
        |value: &str, len: usize| value.len() == len && value.bytes().all(|b| b.is_ascii_digit());
    let location = match parts.as_slice() {
        ["proposals", _] => RecordLocation::Flat,
        ["proposals", "open", _] => RecordLocation::Open,
        ["proposals", folder, year, month, _]
            if digits(year, 4) && digits(month, 2) && ("01"..="12").contains(month) =>
        {
            match ProposalStatus::from_folder(folder) {
                Some(status @ (ProposalStatus::Accepted | ProposalStatus::Dismissed)) => {
                    RecordLocation::Closed {
                        status,
                        year: (*year).to_string(),
                        month: (*month).to_string(),
                    }
                }
                Some(ProposalStatus::Open) | None => {
                    return Err(invalid("proposals/<status>/<id>.md path"));
                }
            }
        }
        _ => return Err(invalid("proposals/<status>/<id>.md path")),
    };
    let name = parts.last().copied().unwrap_or_default();
    let id = name.strip_suffix(".md").unwrap_or_default();
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(invalid("proposal record filename"));
    }
    Ok(RecordPath {
        id: id.to_string(),
        location,
    })
}

/// Proposal records are Markdown pages in a status folder (or the legacy
/// flat layout) under the proposal root.
pub fn validate_record_path(relative: &std::path::Path) -> Result<()> {
    parse_record_path(relative).map(|_| ())
}

/// The page path for `id` in `status`. A closed page is dated by `closed_at`
/// (RFC 3339, UTC), so each closed folder holds one month.
pub fn record_path(id: &str, status: ProposalStatus, closed_at: &str) -> Result<String> {
    let path = match status {
        ProposalStatus::Open => format!("proposals/open/{id}.md"),
        ProposalStatus::Accepted | ProposalStatus::Dismissed => {
            let (year, month) = closed_month(closed_at).ok_or_else(|| invalid("closing time"))?;
            format!("proposals/{}/{year}/{month}/{id}.md", status.folder())
        }
    };
    parse_record_path(std::path::Path::new(&path))?;
    Ok(path)
}

/// The `YYYY` and `MM` of an RFC 3339 timestamp.
#[must_use]
pub fn closed_month(at: &str) -> Option<(String, String)> {
    timestamp(at)?;
    let year = at.get(0..4)?;
    let month = at.get(5..7)?;
    Some((year.to_string(), month.to_string()))
}

/// Heading of the section a closed page carries its outcome under.
pub const OUTCOME_HEADING: &str = "## Outcome";

/// Append an outcome section to a page that has none; a page that already
/// records its outcome is returned unchanged.
#[must_use]
pub fn with_outcome(page: &str, outcome: &str) -> String {
    if page.lines().any(|line| line == OUTCOME_HEADING) {
        return page.to_string();
    }
    let mut text = page.trim_end().to_string();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(OUTCOME_HEADING);
    text.push_str("\n\n");
    text.push_str(outcome.trim_end());
    text.push('\n');
    text
}

// Numeric five-field cron, with lists, ranges and positive steps. Restricting
// proposals to this explicit subset avoids discovering invalid triggers after
// reserving acceptance. Orbit remains the callback's schema authority.
fn valid_cron(cron: &str) -> bool {
    let fields: Vec<_> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    fields
        .iter()
        .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)])
        .all(|(field, (min, max))| {
            field.split(',').all(|part| {
                let (range, step) = part
                    .split_once('/')
                    .map_or((part, None), |(range, step)| (range, Some(step)));
                if step.is_some_and(|value| {
                    value.parse::<u32>().ok().is_none_or(|n| n == 0 || n > max)
                }) {
                    return false;
                }
                let number =
                    |value: &str| value.parse::<u32>().ok().filter(|n| *n >= min && *n <= max);
                if range == "*" {
                    true
                } else if let Some((start, end)) = range.split_once('-') {
                    number(start)
                        .zip(number(end))
                        .is_some_and(|(start, end)| start <= end)
                } else {
                    number(range).is_some()
                }
            })
        })
}

/// First line of an acceptance receipt; legacy approvals without it remain readable.
pub const APPROVAL_DIGEST_PREFIX: &str = "Office proposal payload sha256: ";
