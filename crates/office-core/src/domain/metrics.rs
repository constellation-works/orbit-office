//! Deterministic analytics over recorded Orbit work and Library approvals.
//! Windows are half-open UTC dates; weeks start Monday. No model estimates.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use super::artifact::{Approver, Artifact};
use super::knowledge::frontmatter;
use super::source::retention_until;
use crate::{Error, Result};

/// Refuse incomplete evidence rather than reporting invented zeroes.
#[must_use]
pub fn invalid_data(field: &str) -> Error {
    Error::Library {
        code: "metrics_data_invalid",
        message: format!("analytics requires valid recorded {field}"),
    }
}

/// A validated half-open window of UTC calendar dates.
#[derive(Clone, Debug, Serialize)]
pub struct Window {
    /// Inclusive UTC date.
    pub since: String,
    /// Exclusive UTC date.
    pub until: String,
    #[serde(skip)]
    start: i64,
    #[serde(skip)]
    end: i64,
}

impl Window {
    /// Validate real calendar dates and bound requests to ten years.
    pub fn new(since: &str, until: &str) -> Result<Self> {
        let start = date_seconds(since);
        let end = date_seconds(until);
        match (start, end) {
            (Some(start), Some(end)) if start < end && end - start <= 3660 * 86400 => Ok(Self {
                since: since.into(), until: until.into(), start, end,
            }),
            _ => Err(Error::InvalidArgument("since and until must be YYYY-MM-DD dates with since < until, at most ten years apart (until is exclusive)".into())),
        }
    }

    /// Whether a recorded RFC 3339 timestamp belongs to this window.
    pub fn contains(&self, at: &str) -> Result<bool> {
        Ok(self.includes(timestamp(at).ok_or_else(|| invalid_data("timestamp"))?))
    }

    fn includes(&self, at: i64) -> bool {
        self.start <= at && at < self.end
    }
}

/// RFC 3339 timestamp to UTC whole seconds. Fractions cannot cross a date
/// boundary, so retaining whole seconds is exact for these calendar windows.
#[must_use]
pub fn timestamp(text: &str) -> Option<i64> {
    let date = date_seconds(text.get(..10)?)?;
    let bytes = text.as_bytes();
    if bytes.get(10) != Some(&b'T') || bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let hour: i64 = digits(text.get(11..13)?)?;
    let minute: i64 = digits(text.get(14..16)?)?;
    let second: i64 = digits(text.get(17..19)?)?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut suffix = text.get(19..)?;
    if let Some(fraction) = suffix.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return None;
        }
        suffix = fraction.get(count..)?;
    }
    let offset = if suffix == "Z" {
        0
    } else {
        if suffix.len() != 6
            || !matches!(suffix.as_bytes()[0], b'+' | b'-')
            || suffix.as_bytes()[3] != b':'
        {
            return None;
        }
        let hours = digits(suffix.get(1..3)?)?;
        let minutes = digits(suffix.get(4..6)?)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        (hours * 3600 + minutes * 60) * if suffix.starts_with('-') { -1 } else { 1 }
    };
    Some(date + hour * 3600 + minute * 60 + second - offset)
}

fn digits(text: &str) -> Option<i64> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Seconds at UTC midnight of a real `YYYY-MM-DD` date.
pub(crate) fn date_seconds(text: &str) -> Option<i64> {
    if text.len() != 10 || text.as_bytes()[4] != b'-' || text.as_bytes()[7] != b'-' {
        return None;
    }
    let year = digits(text.get(..4)?)?;
    let month = digits(text.get(5..7)?)?;
    let day = digits(text.get(8..10)?)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let maximum = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if year == 0 || day < 1 || day > maximum {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    Some((era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468) * 86400)
}

/// Strict frontmatter values for period selection (uses the Library parser).
pub fn metadata(text: &str) -> Result<Map<String, Value>> {
    let (header, _) = frontmatter::split(text)?;
    Ok(frontmatter::parse(header)?.into_iter().collect())
}

/// One recorded state transition, independent of Orbit's storage.
#[derive(Clone, Debug)]
pub struct Transition {
    /// UTC seconds of the transition.
    pub at: i64,
    /// Recorded previous state, if the producer supplied it.
    pub from: Option<String>,
    /// Recorded destination state.
    pub to: String,
    /// Original timestamp for citations.
    pub timestamp: String,
}

/// Typed Orbit work facts; only Office owner/proposal records enter analytics.
#[derive(Clone, Debug)]
pub struct Task {
    /// Orbit identity.
    pub id: String,
    /// Recorded title.
    pub title: String,
    /// Recorded tags, including ownership and optional estimate/cluster.
    pub tags: Vec<String>,
    /// Creation timestamp in UTC seconds.
    pub created: i64,
    /// Current recorded lifecycle state.
    pub status: String,
    /// Chronologically ordered state transitions.
    pub transitions: Vec<Transition>,
    /// Estimate from a tag, else the documented default by Orbit task type.
    pub minutes: u32,
    /// Actual task artifact paths (including attached context).
    pub outputs: Vec<String>,
}

impl Task {
    /// Parse the projected task-list record. Missing/malformed facts refuse;
    /// unrelated Orbit tasks are ignored before parsing their work metadata.
    pub fn from_record(value: &Value) -> Result<Option<Self>> {
        let tags: Vec<String> = serde_json::from_value(
            value
                .get("tags")
                .cloned()
                .ok_or_else(|| invalid_data("task.tags"))?,
        )
        .map_err(|_| invalid_data("task.tags"))?;
        if !tags.iter().any(|tag| {
            matches!(
                tag.as_str(),
                "owner:you" | "owner:agent" | "owner:routine" | "proposal"
            )
        }) {
            return Ok(None);
        }
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid_data(&format!("task.{key}")))
        };
        let created =
            timestamp(text("created_at")?).ok_or_else(|| invalid_data("task.created_at"))?;
        let status = text("status")?.to_string();
        if !valid_status(&status) {
            return Err(invalid_data("task.status"));
        }
        let mut transitions = Vec::new();
        for event in value
            .get("history")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_data("task.history"))?
        {
            if let Some(to) = event.get("to_status").and_then(Value::as_str) {
                let stamp = event
                    .get("at")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_data("history.at"))?;
                if !valid_status(to) {
                    return Err(invalid_data("history.to_status"));
                }
                transitions.push(Transition {
                    at: timestamp(stamp).ok_or_else(|| invalid_data("history.at"))?,
                    from: event
                        .get("from_status")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    to: to.into(),
                    timestamp: stamp.into(),
                });
            }
        }
        transitions.sort_by_key(|event| event.at);
        if matches!(status.as_str(), "done" | "rejected" | "archived") && transitions.is_empty() {
            return Err(invalid_data(
                "task status history (updated_at is not a completion time)",
            ));
        }
        let kind = text("type")?;
        let mut minutes = match kind {
            "feature" => 60,
            "bug" | "refactor" => 30,
            "chore" => 15,
            _ => return Err(invalid_data("task.type")),
        };
        let estimates: Vec<_> = tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("estimate-minutes:"))
            .collect();
        if estimates.len() > 1 {
            return Err(invalid_data("estimate-minutes tag"));
        }
        if let Some(estimate) = estimates.first() {
            minutes = estimate
                .parse::<u32>()
                .ok()
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(|| invalid_data("estimate-minutes tag"))?;
        }
        let outputs = value
            .get("artifacts")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_data("task.artifacts"))?
            .iter()
            .map(|file| {
                file.get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| invalid_data("artifact.path"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(Self {
            id: text("id")?.into(),
            title: text("title")?.into(),
            tags,
            created,
            status,
            transitions,
            minutes,
            outputs,
        }))
    }

    /// Exact tag membership.
    #[must_use]
    pub fn tagged(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Task state just before the window end, reconstructed from transitions.
    #[must_use]
    pub fn state_before(&self, end: i64) -> &str {
        self.transitions
            .iter()
            .rev()
            .find(|e| e.at < end)
            .map(|e| e.to.as_str())
            .unwrap_or_else(|| {
                self.transitions
                    .first()
                    .and_then(|e| e.from.as_deref())
                    .unwrap_or(&self.status)
            })
    }

    /// Latest entry into `status` before the window ends.
    #[must_use]
    pub fn entered(&self, status: &str, window: &Window) -> Option<&Transition> {
        self.transitions
            .iter()
            .rev()
            .find(|e| e.at < window.end && e.to == status)
    }

    /// Completion belonging to this window, excluding proposals/housekeeping.
    #[must_use]
    pub fn completion(&self, window: &Window, artifacts: &[Artifact]) -> Option<&Transition> {
        if self.tagged("proposal")
            || self.digest(artifacts)
            || self.state_before(window.end) != "done"
        {
            return None;
        }
        self.entered("done", window)
            .filter(|e| window.includes(e.at))
    }

    fn digest(&self, artifacts: &[Artifact]) -> bool {
        self.tagged("series:digest-daily")
            || artifacts
                .iter()
                .any(|a| a.series == "digest-daily" && a.task == self.id)
    }
}

fn valid_status(status: &str) -> bool {
    matches!(
        status,
        "proposed"
            | "backlog"
            | "in-progress"
            | "in_progress"
            | "review"
            | "done"
            | "blocked"
            | "archived"
            | "rejected"
            | "someday"
    )
}

/// Numerator, denominator and an undefined ratio for an empty population.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Share {
    /// Matching observations.
    pub numerator: u32,
    /// Eligible observations.
    pub denominator: u32,
    /// `None` when there is no eligible observation.
    pub ratio: Option<f64>,
}
impl Share {
    fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
            ratio: (denominator > 0).then(|| f64::from(numerator) / f64::from(denominator)),
        }
    }
}

/// Automation counts and estimated minutes, keeping the denominators visible.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Automation {
    /// Completed agent work plus routine outputs.
    pub automated_count: u32,
    /// Completed user work.
    pub manual_count: u32,
    /// All completed work, excluding daily digests and proposals.
    pub total_count: u32,
    /// Recorded/default minutes of automated work.
    pub automated_minutes: u32,
    /// Recorded/default minutes of manual work.
    pub manual_minutes: u32,
    /// All work's estimated minutes.
    pub total_minutes: u32,
    /// Automated count divided by all completed work.
    pub count_ratio: Option<f64>,
    /// Automated minutes divided by all estimated minutes.
    pub minutes_ratio: Option<f64>,
}

/// One evidenced conversion of a manually performed work cluster.
#[derive(Clone, Debug, Serialize)]
pub struct Conversion {
    /// Shared explicit cluster tag value.
    pub cluster: String,
    /// Accepted routine/auto-task proposal.
    pub proposal: String,
    /// Prior completed manual work in this cluster.
    pub manual_tasks: Vec<String>,
}

/// A user to-do still open at the end of the period.
#[derive(Clone, Debug, Serialize)]
pub struct OpenTask {
    /// Orbit identity.
    pub id: String,
    /// Recorded title.
    pub title: String,
    /// Age at the exclusive period end in whole days.
    pub age_days: u32,
}

/// Proposal outcomes in the period.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Proposals {
    /// Proposals accepted in this window.
    pub accepted: u32,
    /// Proposals dismissed in this window.
    pub dismissed: u32,
    /// Accepted divided by all decisions, undefined when none were made.
    pub ratio: Option<f64>,
}

/// All seven metrics for one period or week.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    /// Automation ratio, by count and minutes.
    pub automation: Automation,
    /// Evidenced manual-to-automation conversions.
    pub conversions: Vec<Conversion>,
    /// Estimated automated minutes divided by sixty.
    pub hours_returned: f64,
    /// User-approved non-digest outputs accepted without edits.
    pub first_pass_acceptance: Share,
    /// Completed agent tasks with an attached runbook.
    pub knowledge_coverage: Share,
    /// Accepted and dismissed proposal outcomes.
    pub proposal_acceptance: Proposals,
    /// Open user work, oldest first.
    pub open_load: Vec<OpenTask>,
}

/// A weekly bucket, clipped to the requested window.
#[derive(Clone, Debug, Serialize)]
pub struct Week {
    /// Inclusive UTC date.
    pub since: String,
    /// Exclusive UTC date.
    pub until: String,
    /// Metrics computed only for this bucket.
    pub metrics: Metrics,
}

/// Period totals and Monday-based weekly buckets.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Inclusive UTC date.
    pub since: String,
    /// Exclusive UTC date.
    pub until: String,
    /// Whole-window metrics.
    pub totals: Metrics,
    /// Calendar weeks intersecting the requested window.
    pub weeks: Vec<Week>,
}

/// Whether the proposal's accepted/dismissed decision belongs to this window.
#[must_use]
pub fn proposal_decision<'a>(
    task: &'a Task,
    window: &Window,
    status: &str,
) -> Option<&'a Transition> {
    if !task.tagged("proposal") || task.state_before(window.end) != status {
        return None;
    }
    task.entered(status, window)
        .filter(|e| window.includes(e.at))
}

/// Compute all metrics with no I/O, model or ambient clock.
pub fn compute(window: &Window, tasks: &[Task], artifacts: &[Artifact]) -> Result<Report> {
    let mut weeks = Vec::new();
    let mut start = window.since.clone();
    while start < window.until {
        let days = date_seconds(&start).ok_or_else(|| invalid_data("week"))? / 86400;
        let to_monday = 7 - (days + 3).rem_euclid(7);
        let next = retention_until(
            &start,
            u32::try_from(to_monday).map_err(|_| invalid_data("week"))?,
        );
        let end = next.min(window.until.clone());
        let bucket = Window::new(&start, &end)?;
        weeks.push(Week {
            since: start.clone(),
            until: end.clone(),
            metrics: calculate(&bucket, tasks, artifacts)?,
        });
        start = end;
    }
    Ok(Report {
        since: window.since.clone(),
        until: window.until.clone(),
        totals: calculate(window, tasks, artifacts)?,
        weeks,
    })
}

fn add(total: &mut u32, count: u32) -> Result<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| invalid_data("metric total (overflow)"))?;
    Ok(())
}

fn calculate(window: &Window, tasks: &[Task], artifacts: &[Artifact]) -> Result<Metrics> {
    let mut result = Metrics::default();
    let mut covered = 0;
    let mut agents = 0;
    for task in tasks {
        if task.completion(window, artifacts).is_some() {
            let automated = task.tagged("owner:agent") || task.tagged("owner:routine");
            if automated {
                add(&mut result.automation.automated_count, 1)?;
                add(&mut result.automation.automated_minutes, task.minutes)?;
            } else {
                add(&mut result.automation.manual_count, 1)?;
                add(&mut result.automation.manual_minutes, task.minutes)?;
            }
            if task.tagged("owner:agent") {
                add(&mut agents, 1)?;
                if task
                    .outputs
                    .iter()
                    .any(|p| p.starts_with("context/knowledge/runbooks/") && p.ends_with(".md"))
                {
                    add(&mut covered, 1)?;
                }
            }
        }
        if task.tagged("owner:you")
            && !task.tagged("proposal")
            && task.created < window.end
            && !matches!(
                task.state_before(window.end),
                "done" | "rejected" | "archived"
            )
        {
            result.open_load.push(OpenTask {
                id: task.id.clone(),
                title: task.title.clone(),
                age_days: u32::try_from((window.end - task.created) / 86400)
                    .map_err(|_| invalid_data("task age"))?,
            });
        }
        if let Some(accepted) = proposal_decision(task, window, "done") {
            add(&mut result.proposal_acceptance.accepted, 1)?;
            if task.tagged("proposal:routine") || task.tagged("proposal:auto_task") {
                for cluster in task.tags.iter().filter_map(|t| t.strip_prefix("cluster:")) {
                    let tag = format!("cluster:{cluster}");
                    // A replacement automation is a proposal decision, but
                    // does not convert an already automated cluster again.
                    let previously_covered = tasks.iter().any(|other| {
                        other.tagged("proposal")
                            && (other.tagged("proposal:routine")
                                || other.tagged("proposal:auto_task"))
                            && other.tagged(&tag)
                            && other.transitions.iter().any(|event| {
                                event.to == "done"
                                    && (event.at, &other.id) < (accepted.at, &task.id)
                            })
                    });
                    if previously_covered {
                        continue;
                    }
                    let mut manual_tasks: Vec<_> = tasks
                        .iter()
                        .filter(|t| {
                            t.tagged("owner:you")
                                && !t.tagged("proposal")
                                && t.tagged(&tag)
                                && t.transitions
                                    .iter()
                                    .any(|e| e.to == "done" && e.at < accepted.at)
                        })
                        .map(|t| t.id.clone())
                        .collect();
                    manual_tasks.sort();
                    manual_tasks.dedup();
                    if !manual_tasks.is_empty()
                        && !result.conversions.iter().any(|c| c.cluster == cluster)
                    {
                        result.conversions.push(Conversion {
                            cluster: cluster.into(),
                            proposal: task.id.clone(),
                            manual_tasks,
                        });
                    }
                }
            }
        }
        if proposal_decision(task, window, "rejected").is_some() {
            add(&mut result.proposal_acceptance.dismissed, 1)?;
        }
    }
    // Non-task routine outputs count once per originating run/task, never per file
    // or promoted version. A completed task already represents that same work.
    let mut first_routine = BTreeMap::new();
    for artifact in artifacts {
        if artifact.series != "digest-daily"
            && artifact.tags.iter().any(|tag| tag.starts_with("routine:"))
            && !tasks.iter().any(|task| task.id == artifact.task)
        {
            let key = (
                timestamp(&artifact.approved.at)
                    .ok_or_else(|| invalid_data("routine approval time"))?,
                artifact.id.as_str(),
            );
            let first = first_routine.entry(artifact.task.as_str()).or_insert(key);
            *first = (*first).min(key);
        }
    }
    let mut unedited = 0;
    let mut approvals = 0;
    for artifact in artifacts {
        if artifact.series == "digest-daily" || !window.contains(&artifact.approved.at)? {
            continue;
        }
        if artifact.approved.by == Approver::User {
            add(&mut approvals, 1)?;
            if !artifact.approved.edited {
                add(&mut unedited, 1)?;
            }
        }
        if first_routine
            .get(artifact.task.as_str())
            .is_some_and(|first| {
                Some(first.0) == timestamp(&artifact.approved.at) && first.1 == artifact.id
            })
        {
            add(&mut result.automation.automated_count, 1)?;
            add(
                &mut result.automation.automated_minutes,
                artifact_minutes(artifact)?,
            )?;
        }
    }
    let automation = &mut result.automation;
    automation.total_count = automation
        .automated_count
        .checked_add(automation.manual_count)
        .ok_or_else(|| invalid_data("count overflow"))?;
    automation.total_minutes = automation
        .automated_minutes
        .checked_add(automation.manual_minutes)
        .ok_or_else(|| invalid_data("minutes overflow"))?;
    automation.count_ratio = Share::new(automation.automated_count, automation.total_count).ratio;
    automation.minutes_ratio =
        Share::new(automation.automated_minutes, automation.total_minutes).ratio;
    result.hours_returned = f64::from(automation.automated_minutes) / 60.0;
    result.knowledge_coverage = Share::new(covered, agents);
    result.first_pass_acceptance = Share::new(unedited, approvals);
    let proposals = &mut result.proposal_acceptance;
    proposals.ratio = Share::new(
        proposals.accepted,
        proposals
            .accepted
            .checked_add(proposals.dismissed)
            .ok_or_else(|| invalid_data("proposal count overflow"))?,
    )
    .ratio;
    result
        .open_load
        .sort_by(|a, b| b.age_days.cmp(&a.age_days).then_with(|| a.id.cmp(&b.id)));
    result.conversions.sort_by(|a, b| a.cluster.cmp(&b.cluster));
    Ok(result)
}

fn artifact_minutes(artifact: &Artifact) -> Result<u32> {
    use super::artifact::ArtifactKind;
    let estimates: Vec<_> = artifact
        .tags
        .iter()
        .filter_map(|t| t.strip_prefix("estimate-minutes:"))
        .collect();
    match estimates.as_slice() {
        [] => Ok(match artifact.kind {
            ArtifactKind::Report | ArtifactKind::Review => 30,
            ArtifactKind::Doc | ArtifactKind::Script => 60,
            ArtifactKind::Draft | ArtifactKind::Data | ArtifactKind::Other => 15,
        }),
        [estimate] => estimate
            .parse::<u32>()
            .ok()
            .filter(|n| *n <= 1_000_000)
            .ok_or_else(|| invalid_data("routine estimate-minutes tag")),
        _ => Err(invalid_data("routine estimate-minutes tag")),
    }
}
