//! Bootstrap previews, digest-bound budgets and the complete Library snapshot.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::endpoint::Capability;
use super::metrics::metadata;
use super::settings::Settings;
use super::source::{parse_source_meta, sha256_hex, source_body, video};
use crate::{Error, Result};

pub mod consolidation;
pub mod extraction;
pub mod status;

/// Bootstrap order; only these phase names are accepted.
pub const PHASES: [&str; 4] = ["docs", "kt", "team", "process"];

/// Refuse unsupported phases before any storage access.
pub fn phase(value: &str) -> Result<&str> {
    if PHASES.contains(&value) {
        Ok(value)
    } else {
        Err(Error::InvalidArgument(
            "phase must be docs, kt, team or process".into(),
        ))
    }
}

/// One complete source and its optional digest sidecar.
#[derive(Clone, Debug, Serialize)]
pub struct Source {
    /// Library-relative source.md path.
    pub path: String,
    /// Full source Markdown.
    pub markdown: String,
    /// Parsed digest.json; malformed or unreadable files must refuse the read.
    pub digest: Option<Value>,
}

impl Source {
    /// Strict metadata, with required source identity and tags.
    pub fn fields(&self) -> Result<serde_json::Map<String, Value>> {
        let fields = metadata(&self.markdown)?;
        if fields
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || fields
                .get("kind")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || fields.get("tags").is_some_and(|v| {
                !v.is_array()
                    || v.as_array()
                        .is_some_and(|a| a.iter().any(|s| !s.is_string()))
            })
        {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "source needs id, kind and string tags",
            ));
        }
        Ok(fields)
    }
    /// Whether the source is tagged for this phase.
    pub fn in_phase(&self, phase: &str) -> Result<bool> {
        Ok(self
            .fields()?
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.contains(&json!(format!("bootstrap:{phase}")))))
    }
    /// OCR-only captures are counted in progress but excluded from inference.
    pub fn needs_ocr(&self) -> Result<bool> {
        let f = self.fields()?;
        Ok(f.get("needs_ocr") == Some(&json!(true))
            || f.get("notes")
                .and_then(Value::as_array)
                .is_some_and(|v| v.contains(&json!("needs_ocr")))
            || f.get("conversion").and_then(Value::as_str) == Some("needs_ocr"))
    }
}

/// Complete file corpus and durable phase ledgers, with deterministic ordering.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    /// Every source, in path order.
    pub sources: Vec<Source>,
    /// Knowledge Markdown keyed by Library-relative path.
    pub pages: BTreeMap<String, String>,
    /// Current previews and execution progress by phase.
    pub batches: BTreeMap<String, Batch>,
}
impl Snapshot {
    /// Compare-and-swap identity, including the ledger and settings.
    pub fn revision(&self, settings: &Settings) -> Result<String> {
        let mut ids = BTreeSet::new();
        for source in &self.sources {
            let fields = source.fields()?;
            if !ids.insert(fields["id"].as_str().unwrap_or_default().to_string()) {
                return Err(video::refusal(
                    "invalid_bootstrap_data",
                    "bootstrap source ids must be unique",
                ));
            }
            if let Some(value) = &source.digest {
                digest(value)?;
            }
        }
        hash(&json!({"snapshot":self,"settings":settings}))
    }
}

/// Counts are upper bounds, including calls that fail or have uncertain results.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// Standard document digest windows and merges for sources without a digest.
    pub digest: usize,
    /// Video audio chunks.
    pub audio: usize,
    /// Video frames.
    pub frames: usize,
    /// Video extraction windows and merges.
    pub extract: usize,
    /// Expected subject groups for careful consolidation.
    pub careful: usize,
}

/// User-visible route for one group of calls; never contains credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// Hosted or private:<kind>.
    pub route: String,
    /// Human label of the selected destination.
    pub label: String,
    /// Model name at that destination.
    pub model: String,
}

/// Fixed call group and its route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallGroup {
    /// digest, audio, frames, extract or careful.
    pub kind: String,
    /// Source id, or null for a consolidation group.
    pub source: Option<String>,
    /// Maximum allowed calls.
    pub calls: usize,
    /// Route selected from all of the group's inputs.
    pub route: Route,
}

/// A cost estimate exists only when every applicable rate is configured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimate {
    /// Configured currency.
    pub currency: String,
    /// Estimated price in that currency.
    pub amount: f64,
    /// Estimated text input/output token allowance (four characters per token).
    pub text_tokens: u64,
}

/// Stored preview bound to source content, settings, routes and estimated calls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// SHA-256 over all other fields.
    pub digest: String,
    /// Bootstrap phase.
    pub phase: String,
    /// Source ids included at preview, including digest inputs carried for overflow.
    pub sources: Vec<String>,
    /// Deferred subjects carried into this preview; omitted on original plans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred_subjects: Vec<String>,
    /// All sources in this preview are marked private when true.
    pub private: bool,
    /// Source/policy snapshot; excludes subsequently written stage-one digests.
    pub inputs_digest: String,
    /// Earlier digest/page evidence used to estimate groups, plus pending digest state.
    pub evidence_digest: String,
    /// Fixed call ceilings.
    pub counts: Counts,
    /// Per-group route and allowance.
    pub groups: Vec<CallGroup>,
    /// Exact previews from the shared video planner.
    pub videos: Vec<video::VideoPlan>,
    /// Null when rates are unset.
    pub estimate: Option<Estimate>,
    /// Applicable missing rate keys, sorted.
    pub rates_unset: Vec<String>,
}

/// A separate, smaller preview for careful calls beyond the initial estimate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    /// Digest of this preview, bound to the original plan and current groups.
    pub digest: String,
    /// Additional calls requiring approval.
    pub calls: usize,
    /// Route for the additional calls.
    pub route: Route,
    /// Optional additional cost.
    pub estimate: Option<Estimate>,
    /// Missing rate keys.
    pub rates_unset: Vec<String>,
}

/// Durable plan, approvals and progress for one phase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    /// Original preview.
    pub plan: Plan,
    /// First matching user confirmation, or null.
    pub confirmed_at: Option<String>,
    /// Consumed calls for each plan group; reserve before inference.
    pub used: Vec<usize>,
    /// Last additional-careful-call preview.
    pub extra: Option<Extra>,
    /// Approval of that additional preview.
    pub extra_confirmed_at: Option<String>,
    /// Consumed additional calls.
    pub extra_used: usize,
    /// Subjects currently being consolidated, set by the consolidation runner.
    pub consolidating: Vec<String>,
    /// Subjects deferred by the page cap, set by the consolidation runner.
    pub overflow: Vec<String>,
}

/// One confirmed batch awaiting consolidation, for routine fan-out.
/// `batch_id` is the ledger key (one batch per phase).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PendingBatch {
    /// Ledger key of the batch (its phase name).
    pub batch_id: String,
    /// Bootstrap phase the batch belongs to.
    pub phase: String,
}

impl Batch {
    /// Whether consolidation has not started for this batch: it is
    /// confirmed and neither the consolidation runner's progress record
    /// (`consolidating`/`overflow`) nor any careful call (original or
    /// additional) has been recorded against it.
    #[must_use]
    pub fn is_pending_consolidation(&self) -> bool {
        if self.confirmed_at.is_none()
            || !self.consolidating.is_empty()
            || !self.overflow.is_empty()
        {
            return false;
        }
        let careful_used = self
            .plan
            .groups
            .iter()
            .zip(&self.used)
            .filter(|(group, _)| group.kind == "careful")
            .map(|(_, used)| *used)
            .sum::<usize>();
        careful_used == 0 && self.extra_used == 0
    }
}

/// Confirmed, unconsolidated batches in phase order, at most `limit`.
#[must_use]
pub fn pending_batches(snapshot: &Snapshot, limit: usize) -> Vec<PendingBatch> {
    PHASES
        .iter()
        .filter_map(|phase| {
            let batch = snapshot.batches.get(*phase)?;
            batch.is_pending_consolidation().then(|| PendingBatch {
                batch_id: (*phase).to_string(),
                phase: batch.plan.phase.clone(),
            })
        })
        .take(limit)
        .collect()
}

/// Mutations share one compare-and-swap and one journaled Library transaction.
#[derive(Clone, Debug)]
pub enum Mutation {
    /// Store/confirm a whole phase preview.
    Plan {
        /// Complete computed preview.
        plan: Box<Plan>,
        /// Digest the user saw; absent for preview.
        confirm: Option<String>,
        /// Call timestamp.
        at: String,
    },
    /// Store/confirm additional careful calls computed from stage-one digests.
    Extra {
        /// Phase to refine.
        phase: String,
        /// Digest of the smaller preview; absent for preview.
        confirm: Option<String>,
        /// Call timestamp.
        at: String,
    },
    /// Consume a call slot durably before inference.
    Reserve {
        /// Phase to execute.
        phase: String,
        /// Approved plan or extra-preview digest.
        digest: String,
        /// Index of the original call group (zero for additional careful calls).
        group: usize,
        /// Consume the separate additional careful budget.
        extra: bool,
    },
    /// Record active consolidation and deferred subject groups.
    Progress {
        /// Phase being processed.
        phase: String,
        /// Active subject ids.
        consolidating: Vec<String>,
        /// Subject ids deferred by the page cap.
        overflow: Vec<String>,
    },
}

/// A prepared write: entire ledger plus any source privacy updates.
pub struct Write {
    /// Ledgers to persist in .office/bootstrap.json.
    pub batches: BTreeMap<String, Batch>,
    /// Library-relative source documents changed in this transaction.
    pub sources: BTreeMap<String, String>,
    /// Updated phase returned to the caller.
    pub batch: Batch,
}

fn hash(value: &Value) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(value).map_err(|_| {
        video::refusal("invalid_bootstrap_data", "cannot encode bootstrap state")
    })?))
}
fn signed(plan: &Plan) -> Result<String> {
    let mut value = serde_json::to_value(plan)
        .map_err(|_| video::refusal("invalid_bootstrap_data", "cannot encode batch plan"))?;
    value["digest"] = json!("");
    hash(&value)
}

/// Normalize names and aliases mechanically, independent of locale.
#[must_use]
pub fn normalize(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Validate the shared digest shape before using it as evidence.
pub fn digest(value: &Value) -> Result<()> {
    if !value.is_object() || !value.get("summary").is_some_and(Value::is_string) {
        return Err(video::refusal(
            "invalid_bootstrap_data",
            "digest needs an object and summary",
        ));
    }
    for field in ["subjects", "claims", "procedures", "terms", "refs"] {
        if !value.get(field).is_some_and(Value::is_array) {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "digest lists must be arrays",
            ));
        }
    }
    for subject in value["subjects"].as_array().into_iter().flatten() {
        if subject
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || subject
                .get("template")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || subject.get("aliases").is_some_and(|v| {
                v.as_array()
                    .is_none_or(|a| a.iter().any(|s| !s.is_string()))
            })
        {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "digest subject needs name, template and string aliases",
            ));
        }
    }
    for procedure in value["procedures"].as_array().into_iter().flatten() {
        if procedure
            .get("title")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "digest procedure needs a title",
            ));
        }
    }
    Ok(())
}

/// Group subject names and aliases transitively, then return canonical names.
pub fn subjects(values: impl Iterator<Item = Value>) -> Vec<BTreeSet<String>> {
    let mut groups: Vec<BTreeSet<String>> = Vec::new();
    for subject in values {
        let mut names = BTreeSet::new();
        if let Some(name) = subject["name"].as_str() {
            names.insert(normalize(name));
        }
        for alias in subject["aliases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            names.insert(normalize(alias));
        }
        names.remove("");
        if names.is_empty() {
            continue;
        }
        let mut index = 0;
        while index < groups.len() {
            if !names.is_disjoint(&groups[index]) {
                names.extend(groups.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        groups.push(names);
    }
    groups.sort();
    groups
}

fn route(settings: &Settings, private: bool, careful: bool) -> Result<Route> {
    if private {
        let endpoint = settings.open_weights.as_ref().ok_or_else(|| {
            video::refusal(
                "private_endpoint_unconfigured",
                "configure the private endpoint before planning private material",
            )
        })?;
        if !endpoint.accepts(Capability::Text) || endpoint.verified_at.is_none() {
            return Err(video::refusal(
                "private_endpoint_unverified",
                "check the private endpoint's text capability before planning",
            ));
        }
        Ok(Route {
            route: format!("private:{}", endpoint.kind.as_str()),
            label: format!(
                "Private: {}{}",
                endpoint.label,
                if endpoint.kind.as_str() == "provider" {
                    " (third party)"
                } else {
                    ""
                }
            ),
            model: endpoint.model.clone(),
        })
    } else {
        Ok(Route {
            route: "hosted".into(),
            label: "Meta hosted API".into(),
            model: if careful {
                &settings.models.careful
            } else {
                "muse-spark-1.3"
            }
            .into(),
        })
    }
}

fn marked(markdown: &str) -> Result<String> {
    let (header, body) = super::knowledge::frontmatter::split(markdown)?;
    // Remove a block value together with its key; preserve every unrelated byte.
    let mut lines = Vec::new();
    let mut skipping = false;
    for line in header.lines() {
        if !line.starts_with(char::is_whitespace) {
            skipping = line.starts_with("sensitivity:");
        }
        if !skipping {
            lines.push(line);
        }
    }
    Ok(format!(
        "---\n{}\nsensitivity: private\n---\n{body}",
        lines.join("\n")
    ))
}

fn input_digest(snapshot: &Snapshot, plan: &Plan, settings: &Settings) -> Result<String> {
    let mut inputs = Vec::new();
    let phase_index = PHASES
        .iter()
        .position(|name| *name == plan.phase)
        .ok_or_else(|| video::refusal("invalid_bootstrap_data", "unsupported batch phase"))?;
    for source in &snapshot.sources {
        let f = source.fields()?;
        let id = f["id"].as_str().unwrap_or_default();
        if plan.sources.iter().any(|s| s == id) {
            let markdown = if plan.private {
                marked(&source.markdown)?
            } else {
                source.markdown.clone()
            };
            // Extraction provenance is an output. Publishing one digest must
            // not revoke the approved allowance for the remaining documents.
            let mut fields = metadata(&markdown)?;
            if let Some(provenance) = fields.get_mut("derived_by").and_then(Value::as_object_mut) {
                provenance.remove("extraction");
                if provenance.is_empty() {
                    fields.remove("derived_by");
                }
            }
            inputs
                .push(json!({"path":source.path,"metadata":fields,"body":source_body(&markdown)}));
            if !plan.deferred_subjects.is_empty()
                && !plan
                    .groups
                    .iter()
                    .any(|group| group.source.as_deref() == Some(id))
                && source.digest.is_some()
            {
                inputs.push(json!({"carried_digest":source.path,"digest":source.digest}));
            }
        } else if source.in_phase(&plan.phase)? && source.digest.is_none() && !source.needs_ocr()? {
            // A newly captured pending source invalidates the whole-phase approval.
            inputs.push(json!({"new_pending":id}));
        } else if source.digest.is_some() {
            let mut relevant = false;
            for phase_name in &PHASES[..=phase_index] {
                relevant |= source.in_phase(phase_name)?;
            }
            if relevant {
                inputs.push(json!({"dependency":source.path,"markdown":source.markdown,"digest":source.digest}));
            }
        }
    }
    // Keep an unset optional demo fact out of the fingerprint. Adding this
    // field to Settings must not invalidate existing bootstrap plans when no
    // simulated history date has been configured.
    let mut settings_value = serde_json::to_value(settings)
        .map_err(|_| video::refusal("invalid_bootstrap_data", "cannot encode settings"))?;
    // Calendar reads do not affect bootstrap inference or its approved budget.
    // Keep existing plans stable when the new optional block is introduced/set.
    if let Some(map) = settings_value.as_object_mut() {
        map.remove("calendar");
    }
    if settings.demo.simulated_until.is_none()
        && let Some(settings) = settings_value.as_object_mut()
    {
        settings.remove("demo");
    }
    if settings.models == super::settings::ModelSettings::default()
        && let Some(map) = settings_value.as_object_mut()
    {
        map.remove("models");
    }
    hash(&json!({"sources":inputs,"settings":settings_value,"ids":plan.sources}))
}
fn evidence_digest(snapshot: &Snapshot) -> Result<String> {
    hash(
        &json!({"digests": snapshot.sources.iter().map(|s| json!({"path":s.path,"digest":s.digest})).collect::<Vec<_>>(),"pages":snapshot.pages}),
    )
}

fn price(
    settings: &Settings,
    tokens: u64,
    image_count: usize,
    audio_seconds: u64,
    text_needed: bool,
) -> Result<(Option<Estimate>, Vec<String>)> {
    let mut unset = Vec::new();
    let mut rate = |name: &str, value: &Option<serde_json::Number>, needed: bool| {
        if needed && value.is_none() {
            unset.push(format!("costs.{name}"));
        }
        value
            .as_ref()
            .and_then(serde_json::Number::as_f64)
            .unwrap_or(0.0)
    };
    let text = rate(
        "text_1k_tokens",
        &settings.costs.text_1k_tokens,
        text_needed,
    );
    let image = rate("image", &settings.costs.image, image_count > 0);
    let audio = rate(
        "audio_minute",
        &settings.costs.audio_minute,
        audio_seconds > 0,
    );
    if settings.costs.currency.is_none() {
        unset.push("costs.currency".into());
    }
    unset.sort();
    let scalar = |n: u64| n.to_string().parse::<f64>().unwrap_or(f64::INFINITY);
    let amount = scalar(tokens) / 1000.0 * text
        + scalar(u64::try_from(image_count).unwrap_or(u64::MAX)) * image
        + scalar(audio_seconds) / 60.0 * audio;
    if !amount.is_finite() {
        return Err(video::refusal(
            "invalid_bootstrap_data",
            "cost estimate exceeds numeric range",
        ));
    }
    let estimate = if unset.is_empty() {
        Some(Estimate {
            currency: settings.costs.currency.clone().unwrap_or_default(),
            amount,
            text_tokens: tokens,
        })
    } else {
        None
    };
    Ok((estimate, unset))
}

/// Plan the phase using the exact shared video previews supplied by local inspection.
pub fn plan(
    snapshot: &Snapshot,
    settings: &Settings,
    phase_name: &str,
    private: bool,
    videos: Vec<video::VideoPlan>,
) -> Result<Plan> {
    phase(phase_name)?;
    let mut plan = Plan {
        digest: String::new(),
        phase: phase_name.into(),
        sources: Vec::new(),
        deferred_subjects: snapshot
            .batches
            .get(phase_name)
            .map_or_else(Vec::new, |batch| {
                if !batch.overflow.is_empty() {
                    batch.overflow.clone()
                } else if batch.used.iter().all(|used| *used == 0) && batch.extra_used == 0 {
                    batch.plan.deferred_subjects.clone()
                } else {
                    Vec::new()
                }
            }),
        private,
        inputs_digest: String::new(),
        evidence_digest: evidence_digest(snapshot)?,
        counts: Counts::default(),
        groups: Vec::new(),
        videos,
        estimate: None,
        rates_unset: Vec::new(),
    };
    let mut expected = plan
        .deferred_subjects
        .iter()
        .map(|name| json!({"name":name}))
        .collect::<Vec<_>>();
    let mut text_tokens = 0_u64;
    let mut audio_seconds = 0_u64;
    let mut all_private = private;
    for source in &snapshot.sources {
        if let Some(value) = &source.digest {
            digest(value)?;
        }
        if !source.in_phase(phase_name)?
            || (source.digest.is_some() && plan.deferred_subjects.is_empty())
            || source.needs_ocr()?
        {
            continue;
        }
        let fields = source.fields()?;
        let meta = parse_source_meta(&source.markdown);
        let is_private = private || meta.sensitivity == "private";
        all_private |= is_private;
        plan.sources.push(meta.id.clone());
        if source.digest.is_some() {
            continue;
        } // Carry-over needs synthesis only.
        let group_route = route(settings, is_private, false)?;
        if fields["kind"] == "video" {
            let preview = plan
                .videos
                .iter()
                .find(|p| p.id == meta.id)
                .ok_or_else(|| {
                    video::refusal("invalid_bootstrap_data", "video preview is missing")
                })?;
            if preview.sensitivity != if is_private { "private" } else { "normal" }
                || preview.settings_digest != video::settings_digest(settings)?
            {
                return Err(video::refusal(
                    "plan_changed",
                    "video policy changed during batch preview",
                ));
            }
            plan.counts.audio += preview.counts.audio;
            plan.counts.frames += preview.counts.frames;
            plan.counts.extract += preview.counts.extract;
            if preview.transcribe_audio {
                audio_seconds += preview.duration_ms.div_ceil(1000);
            }
            let timeline_tokens = u64::try_from(source_body(&source.markdown).chars().count())
                .unwrap_or(0)
                .div_ceil(4)
                + u64::try_from(preview.counts.frames).unwrap_or(0) * 256
                + if preview.transcribe_audio {
                    preview.duration_ms.div_ceil(60_000) * 200
                } else {
                    0
                };
            text_tokens += timeline_tokens.max(1) * if preview.counts.extract > 1 { 2 } else { 1 };
            for (kind, count) in [
                ("audio", preview.counts.audio),
                ("frames", preview.counts.frames),
                ("extract", preview.counts.extract),
            ] {
                if count > 0 {
                    plan.groups.push(CallGroup {
                        kind: kind.into(),
                        source: Some(meta.id.clone()),
                        calls: count,
                        route: group_route.clone(),
                    });
                }
            }
        } else {
            let windows = extraction::windows(&source_body(&source.markdown));
            let calls = extraction::calls(windows.len());
            plan.counts.digest += calls;
            text_tokens += windows
                .iter()
                .map(|text| u64::try_from(text.chars().count()).unwrap_or(0).div_ceil(4))
                .sum::<u64>()
                + u64::try_from(calls).unwrap_or(0) * 1024
                + if windows.len() > 1 {
                    u64::try_from(windows.len()).unwrap_or(0) * 1024
                } else {
                    0
                };
            plan.groups.push(CallGroup {
                kind: "digest".into(),
                source: Some(meta.id),
                calls,
                route: group_route,
            });
        }
        for line in source_body(&source.markdown)
            .lines()
            .filter(|line| line.starts_with('#'))
        {
            let heading = line.trim_start_matches('#').trim();
            if !heading.is_empty() {
                expected.push(json!({"name":heading}));
            }
        }
    }
    // Earlier phases provide expected names; alias components prevent duplicate calls.
    let phase_index = PHASES.iter().position(|p| *p == phase_name).unwrap_or(0);
    for source in &snapshot.sources {
        if let Some(value) = &source.digest {
            let earlier = PHASES[..phase_index]
                .iter()
                .any(|p| source.in_phase(p).unwrap_or(false));
            if earlier {
                expected.extend(value["subjects"].as_array().into_iter().flatten().cloned());
            }
            if earlier || source.in_phase(phase_name)? {
                all_private |= parse_source_meta(&source.markdown).sensitivity == "private";
            }
        }
    }
    plan.counts.careful = if plan.sources.is_empty() {
        0
    } else {
        subjects(expected.into_iter()).len().max(1)
    };
    if plan.counts.careful > 0 {
        plan.groups.push(CallGroup {
            kind: "careful".into(),
            source: None,
            calls: plan.counts.careful,
            route: route(settings, all_private, true)?,
        });
    }
    text_tokens += u64::try_from(plan.counts.careful).unwrap_or(0) * 2048;
    (plan.estimate, plan.rates_unset) = price(
        settings,
        text_tokens,
        plan.counts.frames,
        audio_seconds,
        !plan.groups.is_empty(),
    )?;
    plan.inputs_digest = input_digest(snapshot, &plan, settings)?;
    plan.digest = signed(&plan)?;
    Ok(plan)
}

/// Validate a confirmed plan against the current source and settings snapshot.
pub fn verify_plan(snapshot: &Snapshot, settings: &Settings, plan: &Plan) -> Result<()> {
    if plan.digest != signed(plan)? || plan.inputs_digest != input_digest(snapshot, plan, settings)?
    {
        return Err(video::refusal(
            "plan_changed",
            "batch sources, settings or route changed; preview and confirm again",
        ));
    }
    Ok(())
}

fn extra(snapshot: &Snapshot, settings: &Settings, batch: &Batch) -> Result<Extra> {
    let mut values = Vec::new();
    let mut runbooks = Vec::new();
    let mut private = batch.plan.private
        || batch
            .plan
            .groups
            .iter()
            .any(|group| group.kind == "careful" && group.route.route.starts_with("private:"));
    for source in &snapshot.sources {
        if source.in_phase(&batch.plan.phase)? {
            let value = source.digest.as_ref().ok_or_else(|| {
                video::refusal(
                    "digests_incomplete",
                    "finish the batch digests before previewing additional careful calls",
                )
            })?;
            digest(value)?;
            let meta = source.fields()?;
            let procedures = value["procedures"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|raw| {
                    consolidation::procedure(
                        raw,
                        source,
                        meta["id"].as_str().unwrap_or_default(),
                        &value["as_of"],
                    )
                })
                .collect::<Vec<_>>();
            for subject in consolidation::candidate_subjects(value, &procedures) {
                if subject["template"] == "runbook" {
                    runbooks.push(subject);
                } else {
                    values.push(subject);
                }
            }
            private |= parse_source_meta(&source.markdown).sensitivity == "private";
        }
    }
    let groups = subjects(values.into_iter());
    let runbooks = subjects(runbooks.into_iter());
    let calls = (groups.len() + runbooks.len()).saturating_sub(batch.plan.counts.careful);
    let route = route(settings, private, true)?;
    let (estimate, rates_unset) = price(
        settings,
        u64::try_from(calls).unwrap_or(0) * 2048,
        0,
        0,
        calls > 0,
    )?;
    let digest = hash(
        &json!({"plan":batch.plan.digest,"evidence":evidence_digest(snapshot)?,"groups":groups,"runbooks":runbooks,"route":route,"estimate":estimate,"rates_unset":rates_unset}),
    )?;
    Ok(Extra {
        digest,
        calls,
        route,
        estimate,
        rates_unset,
    })
}

/// Apply under the Library writer lock after checking Snapshot::revision.
pub fn apply(snapshot: &Snapshot, settings: &Settings, mutation: Mutation) -> Result<Write> {
    let mut batches = snapshot.batches.clone();
    let mut sources = BTreeMap::new();
    let phase_name;
    match mutation {
        Mutation::Plan { plan, confirm, at } => {
            phase_name = phase(&plan.phase)?.to_string();
            verify_plan(snapshot, settings, &plan)?;
            if plan.evidence_digest != evidence_digest(snapshot)? {
                return Err(video::refusal(
                    "plan_changed",
                    "batch digest evidence changed; preview again",
                ));
            }
            let previous = batches
                .get(&phase_name)
                .filter(|old| old.plan.digest == plan.digest)
                .cloned();
            let mut batch = previous.unwrap_or_else(|| Batch {
                used: vec![0; plan.groups.len()],
                plan: *plan,
                confirmed_at: None,
                extra: None,
                extra_confirmed_at: None,
                extra_used: 0,
                consolidating: Vec::new(),
                overflow: Vec::new(),
            });
            if let Some(digest) = confirm {
                if snapshot
                    .batches
                    .get(&phase_name)
                    .is_none_or(|old| old.plan.digest != digest)
                    || batch.plan.digest != digest
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "confirm the digest of the stored preview",
                    ));
                }
                if batch.confirmed_at.is_none() {
                    batch.confirmed_at = Some(at);
                }
            }
            if batch.plan.private {
                for source in &snapshot.sources {
                    if batch
                        .plan
                        .sources
                        .contains(&parse_source_meta(&source.markdown).id)
                    {
                        let text = marked(&source.markdown)?;
                        if text != source.markdown {
                            sources.insert(source.path.clone(), text);
                        }
                    }
                }
            }
            batches.insert(phase_name.clone(), batch);
        }
        Mutation::Extra {
            phase: name,
            confirm,
            at,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches.get_mut(&phase_name).ok_or_else(|| {
                video::refusal("plan_unconfirmed", "preview and confirm the batch first")
            })?;
            verify_plan(snapshot, settings, &batch.plan)?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch before additional careful calls",
                ));
            }
            let preview = extra(snapshot, settings, batch)?;
            if let Some(digest) = confirm {
                if preview.digest != digest
                    || batch.extra.as_ref().is_none_or(|e| e.digest != digest)
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "additional careful preview changed; confirm its current digest",
                    ));
                }
                if batch.extra_confirmed_at.is_none() {
                    batch.extra_confirmed_at = Some(at);
                }
            } else if batch.extra.as_ref() != Some(&preview) {
                if batch.extra_used > preview.calls {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "revised careful estimate is below consumed calls",
                    ));
                }
                batch.extra_confirmed_at = None;
            }
            batch.extra = Some(preview);
        }
        Mutation::Reserve {
            phase: name,
            digest,
            group,
            extra: additional,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches.get_mut(&phase_name).ok_or_else(|| {
                video::refusal("plan_unconfirmed", "preview and confirm the batch first")
            })?;
            verify_plan(snapshot, settings, &batch.plan)?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch before inference",
                ));
            }
            if additional {
                if group != 0 {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "additional careful calls use group zero",
                    ));
                }
                let preview = extra(snapshot, settings, batch)?;
                if batch.extra_confirmed_at.is_none()
                    || batch.extra.as_ref() != Some(&preview)
                    || digest != preview.digest
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "confirm the current additional careful preview",
                    ));
                }
                if batch.extra_used >= preview.calls {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "additional careful allowance exhausted",
                    ));
                }
                batch.extra_used += 1;
            } else {
                if digest != batch.plan.digest {
                    return Err(video::refusal("plan_changed", "batch digest changed"));
                }
                let allowance = batch
                    .plan
                    .groups
                    .get(group)
                    .ok_or_else(|| video::refusal("plan_exceeded", "unknown batch call group"))?
                    .calls;
                let used = batch.used.get_mut(group).ok_or_else(|| {
                    video::refusal("invalid_bootstrap_data", "invalid call ledger")
                })?;
                if *used >= allowance {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "batch call group exhausted; preview additional careful calls when needed",
                    ));
                }
                *used += 1;
            }
        }
        Mutation::Progress {
            phase: name,
            consolidating,
            overflow,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches
                .get_mut(&phase_name)
                .ok_or_else(|| video::refusal("plan_unconfirmed", "batch is unavailable"))?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch first",
                ));
            }
            batch.consolidating = consolidating
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            batch.overflow = overflow
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
        }
    }
    let batch = batches.get(&phase_name).cloned().ok_or_else(|| {
        video::refusal(
            "invalid_bootstrap_data",
            "batch mutation produced no ledger",
        )
    })?;
    Ok(Write {
        batches,
        sources,
        batch,
    })
}

/// Refuse a corrupted saved ledger instead of granting unbounded or unsigned calls.
pub fn validate_batches(batches: &BTreeMap<String, Batch>) -> Result<()> {
    for (name, batch) in batches {
        phase(name)?;
        if batch.plan.phase != *name
            || batch.plan.digest != signed(&batch.plan)?
            || batch.used.len() != batch.plan.groups.len()
            || batch
                .used
                .iter()
                .zip(&batch.plan.groups)
                .any(|(used, group)| *used > group.calls)
            || (batch.confirmed_at.is_none()
                && (batch.used.iter().any(|n| *n > 0) || batch.extra_confirmed_at.is_some()))
            || batch.extra_used > batch.extra.as_ref().map_or(0, |extra| extra.calls)
            || (batch.extra_used > 0 && batch.extra_confirmed_at.is_none())
            || (batch.extra.is_none() && batch.extra_confirmed_at.is_some())
        {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "bootstrap ledger has inconsistent approvals or call counts",
            ));
        }
    }
    Ok(())
}
