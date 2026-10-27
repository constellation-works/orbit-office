//! Typed Library settings. Parsing is pure; the adapter reads `.office/config.yaml`.
//!
//! The supported YAML subset is a two-level mapping, scalar values, and inline
//! or block string lists. Unsupported YAML constructs are refused, never guessed.

use std::collections::{BTreeMap, BTreeSet};

use office_common::Sensitivity;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::endpoint::{Capability, EndpointKind};
use super::metrics::date_seconds;
use crate::{Error, Result};

const KINDS: &[&str] = &[
    "video",
    "meeting",
    "chat",
    "email",
    "doc",
    "receipt",
    "bill",
    "conversation",
    "note",
];

/// Validated Library settings. Missing keys receive documented policy defaults;
/// there are no timestamps or fabricated observations in this shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Value")]
pub struct Settings {
    /// Read-only calendar policy; no credentials.
    pub calendar: super::calendar::CalendarSettings,
    /// Bounds for approved offline scripts.
    pub scripts: super::script::ScriptBounds,
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
/// Hosted synthesis model selection. No endpoint or credential is accepted here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelSettings {
    /// Model slug for careful cross-source synthesis.
    pub careful: String,
}
impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            careful: "muse-wonder".into(),
        }
    }
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
fn number(value: &Value, key: &str, min: f64, max: f64) -> Result<serde_json::Number> {
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
const OPEN_WEIGHTS_SETTABLE: [&str; 4] = ["endpoint", "model", "kind", "label"];
/// Keys only `office.private-route-check` writes.
const OPEN_WEIGHTS_CHECKED: [&str; 2] = ["capabilities", "verified_at"];
const CHECK_ONLY: &str = "written only by office.private-route-check, never typed in";

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

impl Default for Settings {
    fn default() -> Self {
        Self {
            calendar: Default::default(),
            retention: KINDS
                .iter()
                .map(|kind| {
                    (
                        (*kind).into(),
                        if *kind == "meeting" || *kind == "video" {
                            Some(90)
                        } else {
                            None
                        },
                    )
                })
                .collect(),
            sensitivity: KINDS
                .iter()
                .map(|kind| ((*kind).into(), Sensitivity::Normal))
                .collect(),
            packs: vec!["kt".into(), "meetings".into()],
            video: VideoSettings::default(),
            costs: Costs::default(),
            models: ModelSettings::default(),
            thresholds: SignalThresholds {
                repetition_count: 3,
                window_days: 30,
                stale_days: 90,
                friction_count: 2,
                new_subject_count: 2,
                unused_days: 30,
            },
            open_weights: None,
            demo: DemoSettings::default(),
            scripts: Default::default(),
        }
    }
}

impl Settings {
    /// The bundled default config `office.init` writes, in the documented
    /// YAML subset. Round-trips through [`Settings::from_yaml`] to defaults.
    #[must_use]
    pub fn default_yaml() -> String {
        let defaults = Self::default();
        let mut out = String::from("retention:\n");
        for kind in KINDS {
            match defaults.retention[*kind] {
                Some(days) => out.push_str(&format!("  {kind}: {days}\n")),
                None => out.push_str(&format!("  {kind}: null\n")),
            }
        }
        out.push_str("sensitivity:\n");
        for kind in KINDS {
            out.push_str(&format!(
                "  {kind}: {}\n",
                defaults.sensitivity[*kind].as_str()
            ));
        }
        out.push_str(&format!("packs: [{}]\n", defaults.packs.join(", ")));
        out.push_str("thresholds:\n");
        out.push_str(&format!(
            "  repetition_count: {}\n  window_days: {}\n  stale_days: {}\n  friction_count: {}\n  new_subject_count: {}\n  unused_days: {}\n",
            defaults.thresholds.repetition_count,
            defaults.thresholds.window_days,
            defaults.thresholds.stale_days,
            defaults.thresholds.friction_count,
            defaults.thresholds.new_subject_count,
            defaults.thresholds.unused_days
        ));
        out.push_str(&format!(
            "models:\n  careful: {}\n",
            defaults.models.careful
        ));
        out.push_str("video:\n  max_frames_per_hour: 120\n  scene_threshold: 0.3\ncosts:\n  currency: null\n  audio_minute: null\n  image: null\n  text_1k_tokens: null\nopen_weights: null\n");
        out.push_str(&format!("scripts:\n  timeout_seconds: {}\n  log_bytes: {}\n  output_files: {}\n  output_file_bytes: {}\n", defaults.scripts.timeout_seconds, defaults.scripts.log_bytes, defaults.scripts.output_files, defaults.scripts.output_file_bytes));
        out
    }

    /// Parse the documented YAML subset (JSON mappings are also valid YAML).
    pub fn from_yaml(text: &str) -> Result<Self> {
        Self::try_from(parse_yaml(text)?)
    }

    /// Return one validated policy update, preserving every other setting.
    /// Only leaves are settable, plus the whole `open_weights` block (a
    /// mapping of `endpoint`, `model`, `kind` and `label`, or null); whole
    /// per-kind/threshold maps and secret names are refused separately from
    /// invalid values. `open_weights.capabilities` and `verified_at` are
    /// refused: only [`Settings::with_route_check`] writes them, and changing
    /// the endpoint or model clears them. No mutation on error.
    pub fn with_setting(&self, key: &str, value: Value) -> Result<Self> {
        // Normalize public, caller-constructed settings through the disk boundary
        // first, including policy defaults for missing per-kind entries.
        let mut encoded = encode(&Self::try_from(encode(self)?)?)?;
        if key == "open_weights" || key.starts_with("open_weights.") {
            set_open_weights(&mut encoded, key, value)?;
            return Self::try_from(encoded);
        }
        let target = match key.split_once('.') {
            Some((section, name))
                if [
                    "retention",
                    "sensitivity",
                    "thresholds",
                    "scripts",
                    "video",
                    "costs",
                    "models",
                    "demo",
                    "calendar",
                ]
                .contains(&section) =>
            {
                encoded
                    .get_mut(section)
                    .and_then(Value::as_object_mut)
                    .and_then(|map| map.get_mut(name))
            }
            None if key == "packs" => encoded.get_mut(key),
            _ => None,
        }
        .ok_or_else(|| Error::UnknownSetting { key: key.into() })?;
        *target = value;
        Self::try_from(encoded)
    }

    /// Record what `office.private-route-check` found: the capabilities the
    /// endpoint accepted and when. Refused with `private_route_changed` when
    /// the configured endpoint or model is no longer the one checked, so a
    /// result never lands on an endpoint it was not measured on.
    pub fn with_route_check(
        &self,
        endpoint: &str,
        model: &str,
        capabilities: &[Capability],
        verified_at: &str,
    ) -> Result<Self> {
        let mut next = Self::try_from(encode(self)?)?;
        let block = next
            .open_weights
            .as_mut()
            .filter(|block| {
                block.endpoint.trim_end_matches('/') == endpoint.trim_end_matches('/')
                    && block.model == model
            })
            .ok_or(Error::PrivateRouteChanged)?;
        let mut capabilities = capabilities.to_vec();
        capabilities.sort_unstable();
        capabilities.dedup();
        block.capabilities = Some(capabilities);
        block.verified_at = Some(verified_at.into());
        next.validate()?;
        Ok(next)
    }

    /// Canonical config text: a JSON mapping, which is also valid YAML and
    /// round-trips without losing escaped strings. Formatting/comments from
    /// hand-edited YAML are not retained. Validate before persisting it.
    pub fn to_yaml(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map(|text| format!("{text}\n"))
            .map_err(|_| invalid("config", "cannot encode settings"))
    }

    /// Validate values constructed or modified by a caller through the same
    /// boundary as disk/deserialization. Call this before using a modified value.
    pub fn validate(&self) -> Result<()> {
        Self::try_from(
            serde_json::to_value(self).map_err(|_| invalid("config", "cannot encode settings"))?,
        )
        .map(|_| ())
    }
}

impl TryFrom<Value> for Settings {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| invalid("config", "expected a mapping"))?;
        let mut settings = Self::default();
        for (key, value) in map {
            match key.as_str() {
                "retention" | "sensitivity" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a per-kind mapping"))?;
                    for (kind, value) in items {
                        let dotted = format!("{key}.{kind}");
                        if !KINDS.contains(&kind.as_str()) {
                            return Err(invalid(&dotted, "unknown source kind"));
                        }
                        if key == "retention" {
                            let days = if value.is_null() {
                                None
                            } else {
                                Some(positive(value, &dotted, 36500)?)
                            };
                            settings.retention.insert(kind.clone(), days);
                        } else {
                            let sensitivity = value
                                .as_str()
                                .and_then(Sensitivity::parse)
                                .ok_or_else(|| invalid(&dotted, "expected normal or private"))?;
                            settings.sensitivity.insert(kind.clone(), sensitivity);
                        }
                    }
                }
                "packs" => {
                    let list = value
                        .as_array()
                        .ok_or_else(|| invalid(key, "expected a list of pack names"))?;
                    let mut seen = BTreeSet::new();
                    settings.packs.clear();
                    for item in list {
                        let pack = item
                            .as_str()
                            .filter(|p| ["kt", "meetings", "receipts"].contains(p))
                            .ok_or_else(|| invalid(key, "expected kt, meetings or receipts"))?;
                        if !seen.insert(pack) {
                            return Err(invalid(key, "duplicate pack"));
                        }
                        settings.packs.push(pack.into());
                    }
                }
                "thresholds" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let dotted = format!("thresholds.{name}");
                        match name.as_str() {
                            "repetition_count" => {
                                settings.thresholds.repetition_count =
                                    positive(value, &dotted, 10000)?
                            }
                            "window_days" => {
                                settings.thresholds.window_days = positive(value, &dotted, 36500)?
                            }
                            "stale_days" => {
                                settings.thresholds.stale_days = positive(value, &dotted, 36500)?
                            }
                            "friction_count" => {
                                settings.thresholds.friction_count =
                                    positive(value, &dotted, 10000)?
                            }
                            "new_subject_count" => {
                                settings.thresholds.new_subject_count =
                                    positive(value, &dotted, 10000)?
                            }
                            "unused_days" => {
                                settings.thresholds.unused_days = positive(value, &dotted, 36500)?
                            }
                            _ => return Err(invalid(&dotted, "unknown threshold")),
                        }
                    }
                }
                "video" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let key = format!("video.{name}");
                        match name.as_str() {
                            "max_frames_per_hour" => {
                                settings.video.max_frames_per_hour = positive(value, &key, 600)?
                            }
                            "scene_threshold" => {
                                settings.video.scene_threshold = number(value, &key, 0.05, 0.95)?
                            }
                            _ => return Err(invalid(&key, "unknown video setting")),
                        }
                    }
                }
                "models" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let key = format!("models.{name}");
                        if name != "careful" {
                            return Err(invalid(&key, "unknown model tier"));
                        }
                        settings.models.careful = value
                            .as_str()
                            .filter(|v| {
                                !v.is_empty()
                                    && v.len() <= 128
                                    && v.bytes()
                                        .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
                            })
                            .ok_or_else(|| {
                                invalid(&key, "expected a model slug of at most 128 ASCII bytes")
                            })?
                            .into();
                    }
                }
                "costs" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let key = format!("costs.{name}");
                        match name.as_str() {
                            "currency" => {
                                settings.costs.currency = if value.is_null() {
                                    None
                                } else {
                                    Some(value.as_str().filter(|code| code.len() == 3 && code.bytes().all(|c| c.is_ascii_uppercase()))
                                    .ok_or_else(|| invalid(&key, "expected a three-letter uppercase ISO 4217 code or null"))?.into())
                                }
                            }
                            "audio_minute" | "image" | "text_1k_tokens" => {
                                let rate = if value.is_null() {
                                    None
                                } else {
                                    Some(number(value, &key, 0.0, f64::MAX)?)
                                };
                                match name.as_str() {
                                    "audio_minute" => settings.costs.audio_minute = rate,
                                    "image" => settings.costs.image = rate,
                                    "text_1k_tokens" => settings.costs.text_1k_tokens = rate,
                                    _ => return Err(invalid(&key, "unknown cost setting")),
                                }
                            }
                            _ => return Err(invalid(&key, "unknown cost setting")),
                        }
                    }
                }
                "calendar" => {
                    settings.calendar = super::calendar::CalendarSettings::parse(value)?;
                }
                "demo" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let key = format!("demo.{name}");
                        match name.as_str() {
                            "simulated_until" => {
                                settings.demo.simulated_until = simulated_until(value, &key)?
                            }
                            _ => return Err(invalid(&key, "unknown demo setting")),
                        }
                    }
                }
                "scripts" => {
                    settings.scripts = serde_json::from_value(value.clone()).map_err(|_| {
                        invalid("scripts", "expected a closed mapping of integer bounds")
                    })?;
                    settings.scripts.validate()?;
                }
                "open_weights" => settings.open_weights = open_weights(value)?,
                // The pre-batch initializer wrote this unset value. A configured
                // old endpoint cannot be mapped safely to the new route block.
                "open_weights_endpoint" if value.is_null() => {}
                _ => return Err(invalid(key, "unknown setting")),
            }
        }
        Ok(settings)
    }
}

fn encode(settings: &Settings) -> Result<Value> {
    serde_json::to_value(settings).map_err(|_| invalid("config", "cannot encode settings"))
}

/// Apply one `open_weights` update to the encoded settings, before validation.
fn set_open_weights(encoded: &mut Value, key: &str, value: Value) -> Result<()> {
    let Some((_, name)) = key.split_once('.') else {
        if let Some(block) = value.as_object()
            && let Some(checked) = OPEN_WEIGHTS_CHECKED
                .iter()
                .find(|checked| block.contains_key(**checked))
        {
            return Err(invalid(&format!("open_weights.{checked}"), CHECK_ONLY));
        }
        let mut value = value;
        if let (Some(previous), Some(replacement)) = (
            encoded.get("open_weights").and_then(Value::as_object),
            value.as_object_mut(),
        ) {
            let same_route = ["endpoint", "model"].into_iter().all(|name| {
                previous
                    .get(name)
                    .is_some_and(|old| replacement.get(name) == Some(old))
            });
            if same_route {
                for checked in OPEN_WEIGHTS_CHECKED {
                    if let Some(value) = previous.get(checked) {
                        replacement.insert(checked.into(), value.clone());
                    }
                }
            }
        }
        encoded["open_weights"] = value;
        return Ok(());
    };
    if OPEN_WEIGHTS_CHECKED.contains(&name) {
        return Err(invalid(key, CHECK_ONLY));
    }
    if !OPEN_WEIGHTS_SETTABLE.contains(&name) {
        return Err(Error::UnknownSetting { key: key.into() });
    }
    let block = encoded
        .get_mut("open_weights")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            invalid(
                "open_weights",
                "set the whole open_weights block (endpoint, model, kind, label) first",
            )
        })?;
    // A new endpoint or model has not been checked: what the old one
    // accepted says nothing about it.
    if ["endpoint", "model"].contains(&name) && block.get(name) != Some(&value) {
        for checked in OPEN_WEIGHTS_CHECKED {
            block.remove(checked);
        }
    }
    block.insert(name.into(), value);
    Ok(())
}

/// Validate the `open_weights` block (null means unconfigured).
fn open_weights(value: &Value) -> Result<Option<OpenWeights>> {
    if value.is_null() {
        return Ok(None);
    }
    let map = value.as_object().ok_or_else(|| {
        invalid(
            "open_weights",
            "expected a mapping of endpoint, model, kind and label, or null",
        )
    })?;
    if let Some(unknown) = map.keys().find(|key| {
        !OPEN_WEIGHTS_SETTABLE.contains(&key.as_str())
            && !OPEN_WEIGHTS_CHECKED.contains(&key.as_str())
    }) {
        return Err(invalid(
            &format!("open_weights.{unknown}"),
            "unknown open_weights key",
        ));
    }
    let text = |name: &str, max: usize| -> Result<String> {
        let dotted = format!("open_weights.{name}");
        map.get(name)
            .ok_or_else(|| invalid(&dotted, "required in the open_weights block"))?
            .as_str()
            .filter(|text| {
                !text.trim().is_empty()
                    && text.chars().count() <= max
                    && !text.chars().any(char::is_control)
            })
            .map(str::to_string)
            .ok_or_else(|| {
                invalid(
                    &dotted,
                    "expected a nonempty single-line string within the limits in docs/design/office/specs/plugin-surface.md#private-route-field-limits",
                )
            })
    };
    let endpoint = text("endpoint", 2048)?;
    super::endpoint::allow_private_endpoint(&endpoint)
        .map_err(|reason| invalid("open_weights.endpoint", reason))?;
    let model = text("model", 200)?;
    if model.chars().any(char::is_whitespace) {
        return Err(invalid(
            "open_weights.model",
            "expected the model name as the server names it, without whitespace (see docs/design/office/specs/plugin-surface.md#private-route-field-limits)",
        ));
    }
    let kind = EndpointKind::parse(&text("kind", 32)?).ok_or_else(|| {
        invalid(
            "open_weights.kind",
            "expected company, provider or own_hardware",
        )
    })?;
    let label = text("label", 80)?;
    let capabilities = map
        .get("capabilities")
        .map(|value| {
            let list = value.as_array().ok_or_else(|| {
                invalid(
                    "open_weights.capabilities",
                    "expected a list of text, image or audio",
                )
            })?;
            let mut seen = Vec::new();
            for item in list {
                let capability = item.as_str().and_then(Capability::parse).ok_or_else(|| {
                    invalid(
                        "open_weights.capabilities",
                        "expected a list of text, image or audio",
                    )
                })?;
                if seen.contains(&capability) {
                    return Err(invalid("open_weights.capabilities", "duplicate capability"));
                }
                seen.push(capability);
            }
            Ok(seen)
        })
        .transpose()?;
    let verified_at = map
        .get("verified_at")
        .map(|value| {
            value
                .as_str()
                .filter(|text| is_rfc3339(text))
                .map(str::to_string)
                .ok_or_else(|| {
                    invalid("open_weights.verified_at", "expected an RFC 3339 timestamp")
                })
        })
        .transpose()?;
    // The check writes both together; one without the other was typed in.
    match (&capabilities, &verified_at) {
        (Some(_), None) => return Err(invalid("open_weights.capabilities", CHECK_ONLY)),
        (None, Some(_)) => return Err(invalid("open_weights.verified_at", CHECK_ONLY)),
        (Some(_), Some(_)) | (None, None) => {}
    }
    Ok(Some(OpenWeights {
        endpoint,
        model,
        kind,
        label,
        capabilities,
        verified_at,
    }))
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`.
fn is_rfc3339(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    let at = |index: usize, byte: u8| bytes.get(index) == Some(&byte);
    if !(digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && (at(10, b'T') || at(10, b't'))
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19))
    {
        return false;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return false;
        }
        rest = &fraction[count..];
    }
    let offset = rest.as_bytes();
    rest == "Z"
        || rest == "z"
        || (offset.len() == 6
            && matches!(offset[0], b'+' | b'-')
            && offset[1..3].iter().all(u8::is_ascii_digit)
            && offset[3] == b':'
            && offset[4..6].iter().all(u8::is_ascii_digit))
}

/// A real `YYYY-MM-DD` date, or null for no simulated history.
fn simulated_until(value: &Value, key: &str) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|text| date_seconds(text).is_some())
        .map(|text| Some(text.to_string()))
        .ok_or_else(|| invalid(key, "expected a YYYY-MM-DD date or null"))
}

fn positive(value: &Value, key: &str, max: u32) -> Result<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| (1..=max).contains(v))
        .ok_or_else(|| {
            invalid(
                key,
                "expected a positive integer within the documented bound",
            )
        })
}

fn invalid(key: &str, reason: &'static str) -> Error {
    Error::InvalidSetting {
        key: key.into(),
        reason,
    }
}

fn parse_yaml(text: &str) -> Result<Value> {
    if text.trim_start().starts_with('{') {
        return serde_json::from_str(text).map_err(|_| invalid("config", "invalid JSON mapping"));
    }
    let mut root = Map::new();
    let mut section: Option<String> = None;
    for raw in text.lines() {
        // Comments outside quotes only. Quoted hashes and endpoint fragments survive.
        let mut quote = None;
        let mut escaped = false;
        let line = raw.split_once('\r').map_or(raw, |(line, _)| line);
        let mut end = line.len();
        for (i, c) in line.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            // In double quotes `\` escapes the next character; in single quotes `''`
            // is one quote. The escaped character stays inside the value.
            escaped = (quote == Some('"') && c == '\\')
                || (quote == Some('\'') && c == '\'' && line[i + 1..].starts_with('\''));
            if escaped {
                continue;
            }
            // A quote opens only at the start of a value or list item, so an
            // apostrophe inside a word (`Acme's server`) does not hide a comment.
            if (c == '\'' || c == '"')
                && (quote == Some(c)
                    || (quote.is_none()
                        && (i == 0 || matches!(line.as_bytes()[i - 1], b' ' | b'[' | b','))))
            {
                quote = if quote.is_none() { Some(c) } else { None };
            }
            if c == '#'
                && quote.is_none()
                && (i == 0 || line.as_bytes()[i - 1].is_ascii_whitespace())
            {
                end = i;
                break;
            }
        }
        let line = line[..end].trim_end();
        if line.trim().is_empty() || line == "---" || line == "..." {
            continue;
        }
        if line.contains('\t') {
            return Err(invalid(
                section.as_deref().unwrap_or("config"),
                "tabs are not supported",
            ));
        }
        let nested = line.starts_with("  ") && !line.starts_with("   ");
        if !nested && line.starts_with(' ') {
            return Err(invalid(
                section.as_deref().unwrap_or("config"),
                "use two-space indentation",
            ));
        }
        let line = line.trim();
        if nested {
            let parent = section
                .as_deref()
                .ok_or_else(|| invalid("config", "unexpected nested value"))?;
            if let Some(item) = line.strip_prefix("- ") {
                let list = root
                    .get_mut(parent)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid(parent, "unexpected list item"))?;
                list.push(scalar(item, parent)?);
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid(parent, "expected key: value"))?;
            let dotted = format!("{parent}.{key}");
            let map = root
                .get_mut(parent)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| invalid(parent, "unexpected mapping"))?;
            if map.contains_key(key) {
                return Err(invalid(&dotted, "duplicate key"));
            }
            map.insert(key.into(), scalar(value.trim(), &dotted)?);
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid("config", "expected key: value"))?;
            let value = value.trim();
            if root.contains_key(key) {
                return Err(invalid(key, "duplicate key"));
            }
            root.insert(
                key.into(),
                if value.is_empty() {
                    if key == "packs" {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(Map::new())
                    }
                } else {
                    scalar(value, key)?
                },
            );
            section = Some(key.into());
        }
    }
    Ok(Value::Object(root))
}

fn scalar(text: &str, key: &str) -> Result<Value> {
    if text.starts_with('[') {
        let inner = text
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .ok_or_else(|| invalid(key, "unterminated list"))?;
        return Ok(Value::Array(if inner.trim().is_empty() {
            Vec::new()
        } else {
            inner
                .split(',')
                .map(|v| scalar(v.trim(), key))
                .collect::<Result<Vec<_>>>()?
        }));
    }
    if text.starts_with('"') {
        return serde_json::from_str(text).map_err(|_| invalid(key, "invalid quoted string"));
    }
    if text.starts_with('\'') {
        return text
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .map(|v| Value::String(v.replace("''", "'")))
            .ok_or_else(|| invalid(key, "unterminated quoted string"));
    }
    if text == "{}" {
        return Ok(Value::Object(Map::new()));
    }
    if text == "null" || text == "~" {
        return Ok(Value::Null);
    }
    if text.is_empty() || text.starts_with(['&', '*', '!', '{', '|', '>']) {
        return Err(invalid(key, "unsupported YAML value"));
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into())))
}
