use std::collections::BTreeMap;

use office_common::Sensitivity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::helpers::invalid;
use super::model::{Costs, ModelSettings, OpenWeights, SignalThresholds};
use crate::Result;

pub(super) const KINDS: &[&str] = &[
    "video",
    "meeting",
    "chat",
    "email",
    "doc",
    "receipt",
    "bill",
    "conversation",
    "decisions",
];

/// Retired source kinds whose keys a config written before the retirement
/// may still hold. They are ignored on read, never accepted on write.
pub(super) const RETIRED_KINDS: &[&str] = &["note"];

/// Validated Library settings. Missing keys receive documented policy defaults;
/// there are no timestamps or fabricated observations in this shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Value")]
pub struct Settings {
    /// Optional team shared Library, validated on every read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<crate::domain::sharing::SharedSettings>,
    /// Read-only calendar policy; no credentials.
    pub calendar: crate::domain::calendar::CalendarSettings,
    /// Bounds for approved offline scripts.
    pub scripts: crate::domain::script::ScriptBounds,
    /// Days to retain each kind's original; `None` means keep indefinitely.
    pub retention: BTreeMap<String, Option<u32>>,
    /// Default sensitivity for each source kind.
    pub sensitivity: BTreeMap<String, Sensitivity>,
    /// Enabled packs (`kt`, `meetings`, `receipts`).
    pub packs: Vec<String>,
    /// Video keyframe selection bounds.
    pub video: VideoSettings,
    /// Hosted quality-tier choices; private material uses `open_weights.model`.
    pub models: ModelSettings,
    /// Optional preview rates; absent rates never become a fabricated price.
    pub costs: Costs,
    /// Signal policy thresholds.
    pub thresholds: SignalThresholds,
    /// The private route's endpoint; absent means private material is refused.
    pub open_weights: Option<OpenWeights>,
    /// Facts about seeded demo history, recorded by the demo seeder.
    pub demo: DemoSettings,
    /// Session-brief policy.
    pub brief: BriefSettings,
    /// Meeting to-do routing; omitted from the file while default.
    #[serde(skip_serializing_if = "MeetingSettings::is_default")]
    pub meetings: MeetingSettings,
    /// Decision-precedent recency; omitted from the file while default.
    #[serde(skip_serializing_if = "PrecedentSettings::is_default")]
    pub precedents: PrecedentSettings,
}

/// Recency weighting for `office.decision-precedents`
/// (specs/decision-precedents.md, "Configuration").
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrecedentSettings {
    /// Days until a precedent's relevance halves (1–3650, default 90).
    pub half_life_days: u32,
}

impl Default for PrecedentSettings {
    fn default() -> Self {
        Self { half_life_days: 90 }
    }
}

impl PrecedentSettings {
    /// Whether the block is entirely default (and so omitted from the file).
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(super) fn parse(value: &Value) -> Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| invalid("precedents", "expected a mapping of half_life_days"))?;
        let mut out = Self::default();
        for (name, value) in map {
            if name != "half_life_days" {
                return Err(invalid(
                    &format!("precedents.{name}"),
                    "unknown precedent setting",
                ));
            }
            out.half_life_days = value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (1..=3650).contains(n))
                .ok_or_else(|| {
                    invalid(
                        "precedents.half_life_days",
                        "expected an integer from 1 to 3650",
                    )
                })?;
        }
        Ok(out)
    }
}

/// Session-brief policy (specs/sessions.md#session-brief).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BriefSettings {
    /// Extra Orbit workspace selectors whose tasks merge into the brief
    /// (`ws_*`, a registered name, an absolute local checkout path, or a
    /// host-qualified `hm_…/<workspace>` federated selector). Empty means
    /// the office workspace only, as before.
    pub task_workspaces: Vec<String>,
    /// How a SessionStart brief chooses its project handoff
    /// (`brief.handoff`, specs/sessions.md#choosing-the-project). Omitted
    /// from the config when `auto`, the default.
    #[serde(skip_serializing_if = "HandoffMode::is_auto")]
    pub handoff: HandoffMode,
}

/// Meeting to-do routing (specs/kt-videos.md, meeting profile).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MeetingSettings {
    /// Orbit workspace selector that receives agent-owned engineering
    /// to-dos from `office.extract` (profile `meeting`). `None` keeps every
    /// extracted to-do `owner:you` in the office workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_workspace: Option<String>,
}

impl MeetingSettings {
    /// Whether nothing is configured.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `brief.handoff`: whether the SessionStart hook picks the project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HandoffMode {
    /// The hook picks the project from its working directory, else from the
    /// caller's open assignment, else behaves as `manual`.
    #[default]
    Auto,
    /// The hook's brief carries only global handoffs and the projects with
    /// a thread; `/handoff <project>` picks one.
    Manual,
}

impl HandoffMode {
    /// Whether this is the default, `auto`.
    #[must_use]
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

/// Demo-history facts the app's labels read. Records themselves stay unmarked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DemoSettings {
    /// Last date (`YYYY-MM-DD`, inclusive) of seeded history; points dated on
    /// or before it are labelled simulated. `None` means none is recorded.
    pub simulated_until: Option<String>,
}

/// Validated video policy. Decimal settings keep their JSON number type
/// through settings serialization and validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VideoSettings {
    /// Maximum frames per hour, 1 through 600.
    pub max_frames_per_hour: u32,
    /// Scene-change threshold, 0.05 through 0.95.
    pub scene_threshold: serde_json::Number,
}
impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            max_frames_per_hour: 120,
            scene_threshold: serde_json::Number::from_f64(0.3).unwrap_or_else(|| 0.into()),
        }
    }
}
