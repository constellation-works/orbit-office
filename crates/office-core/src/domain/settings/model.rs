use serde::Serialize;
use serde_json::Value;

use super::helpers::invalid;
use crate::Result;
use crate::domain::endpoint::{Capability, EndpointKind};

/// Default hosted model for the standard tier (extraction and vision).
pub const DEFAULT_STANDARD_MODEL: &str = "muse-spark-1.3";
/// Default hosted model for careful cross-source synthesis.
pub const DEFAULT_CAREFUL_MODEL: &str = "muse-wonder";
/// Default hosted transcription model.
pub const DEFAULT_TRANSCRIBE_MODEL: &str = "muse-voice-transcribe-1.0";

/// The hosted route: which OpenAI-compatible endpoint `normal` material
/// goes to and which models it names (specs/models-and-privacy.md, "The
/// hosted route"). Unset keys fall back to Meta's Model API and the Muse
/// models, so an existing config keeps its behaviour; any other endpoint
/// and models may be configured instead. No credential is accepted here:
/// the endpoint's key is the `meta_api_key` secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSettings {
    /// Model slug for careful cross-source synthesis.
    pub careful: String,
    /// Hosted OpenAI-compatible base URL; `None` means
    /// [`crate::domain::endpoint::DEFAULT_HOSTED_ENDPOINT`]. HTTPS unless loopback,
    /// no credentials, query or fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Model slug for the standard tier; `None` means [`DEFAULT_STANDARD_MODEL`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    /// Model slug for transcription; `None` means [`DEFAULT_TRANSCRIBE_MODEL`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcribe: Option<String>,
}
impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            careful: DEFAULT_CAREFUL_MODEL.into(),
            endpoint: None,
            standard: None,
            transcribe: None,
        }
    }
}

impl ModelSettings {
    /// The hosted base URL calls go to: `models.endpoint`, else Meta's.
    #[must_use]
    pub fn hosted_endpoint(&self) -> &str {
        self.endpoint
            .as_deref()
            .unwrap_or(crate::domain::endpoint::DEFAULT_HOSTED_ENDPOINT)
    }

    /// The standard-tier hosted model.
    #[must_use]
    pub fn standard_model(&self) -> &str {
        self.standard.as_deref().unwrap_or(DEFAULT_STANDARD_MODEL)
    }

    /// The hosted transcription model.
    #[must_use]
    pub fn transcribe_model(&self) -> &str {
        self.transcribe
            .as_deref()
            .unwrap_or(DEFAULT_TRANSCRIBE_MODEL)
    }

    /// Whether the hosted endpoint is Meta's Model API, whose one-shot
    /// transcription endpoint (`/asr/transcribe`) hosted audio uses. Any
    /// other endpoint receives audio as Chat Completions `input_audio`.
    #[must_use]
    pub fn hosted_is_meta(&self) -> bool {
        crate::domain::endpoint::host_of(self.hosted_endpoint()).as_deref()
            == Some(crate::domain::endpoint::META_HOST)
    }
}

/// The `models` keys besides `careful`, omitted from the file while unset.
pub(super) const MODEL_OPTIONAL: [&str; 3] = ["endpoint", "standard", "transcribe"];

/// A model slug: nonempty, at most 128 ASCII letters, digits and `-._/:`.
pub(super) fn model_slug(value: &Value, key: &str) -> Result<String> {
    value
        .as_str()
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 128
                && v.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
        })
        .map(str::to_string)
        .ok_or_else(|| invalid(key, "expected a model slug of at most 128 ASCII bytes"))
}

/// Optional per-unit rates for the cost preview.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Costs {
    /// Three-letter uppercase currency code.
    pub currency: Option<String>,
    /// Price per minute transcribed.
    pub audio_minute: Option<serde_json::Number>,
    /// Price per frame read.
    pub image: Option<serde_json::Number>,
    /// Price per thousand estimated text tokens.
    pub text_1k_tokens: Option<serde_json::Number>,
}
pub(super) fn number(value: &Value, key: &str, min: f64, max: f64) -> Result<serde_json::Number> {
    let n = value
        .as_number()
        .filter(|n| {
            n.as_f64()
                .is_some_and(|n| n.is_finite() && (min..=max).contains(&n))
        })
        .ok_or_else(|| invalid(key, "expected a finite number within the documented bounds"))?;
    Ok(n.clone())
}

/// The one private endpoint the user chose (specs/models-and-privacy.md,
/// "The private route"). `capabilities` and `verified_at` are written only
/// by `office.private-route-check`, always together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenWeights {
    /// OpenAI-compatible base URL; HTTPS unless loopback, never `api.meta.ai`.
    pub endpoint: String,
    /// The model as the server names it.
    pub model: String,
    /// Where the endpoint runs.
    pub kind: EndpointKind,
    /// Shown in the app wherever the route is used.
    pub label: String,
    /// What the last check found the endpoint accepts; `None` until checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<Capability>>,
    /// When the last check ran (RFC 3339); `None` until checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
}

impl OpenWeights {
    /// Whether the last check found the endpoint accepts `capability`.
    /// An unchecked endpoint accepts nothing.
    #[must_use]
    pub fn accepts(&self, capability: Capability) -> bool {
        self.capabilities
            .as_deref()
            .is_some_and(|caps| caps.contains(&capability))
    }
}

/// Keys a caller may write in the `open_weights` block.
pub(super) const OPEN_WEIGHTS_SETTABLE: [&str; 4] = ["endpoint", "model", "kind", "label"];
/// Keys only `office.private-route-check` writes.
pub(super) const OPEN_WEIGHTS_CHECKED: [&str; 2] = ["capabilities", "verified_at"];
pub(super) const CHECK_ONLY: &str = "written only by office.private-route-check, never typed in";

/// Positive, bounded signal thresholds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignalThresholds {
    /// Completed similar tasks needed for a repetition signal.
    pub repetition_count: u32,
    /// Repetition window in days.
    pub window_days: u32,
    /// Age in days at which knowledge needs verification.
    pub stale_days: u32,
    /// Recordings of the same friction needed for a recurrence signal.
    pub friction_count: u32,
    /// Sources mentioning a new subject needed for a new-subject signal.
    pub new_subject_count: u32,
    /// Days without an opened output before automation is unused.
    pub unused_days: u32,
}
