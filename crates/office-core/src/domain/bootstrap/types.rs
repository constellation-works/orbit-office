//! Plan, batch and snapshot types for bootstrap.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::digest_ops::{digest, hash};
use crate::domain::metrics::metadata;
use crate::domain::settings::Settings;
use crate::domain::source::video;
use crate::{Error, Result};

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
