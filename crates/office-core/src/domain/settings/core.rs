use office_common::Sensitivity;
use serde_json::{Map, Value};

use super::helpers::{encode, invalid, set_open_weights};
use super::model::{Costs, MODEL_OPTIONAL, ModelSettings, SignalThresholds};
use super::types::{
    BriefSettings, DemoSettings, KINDS, MeetingSettings, PrecedentSettings, Settings, VideoSettings,
};
use super::yaml::parse_yaml;
use crate::domain::endpoint::Capability;
use crate::{Error, Result};

impl Default for Settings {
    fn default() -> Self {
        Self {
            calendar: Default::default(),
            shared: None,
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
            brief: BriefSettings::default(),
            meetings: MeetingSettings::default(),
            precedents: PrecedentSettings::default(),
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
        out.push_str("brief:\n  task_workspaces: []\n");
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
        if key == "shared" {
            encoded["shared"] = value;
            return Self::try_from(encoded);
        }
        if let Some(name) = key.strip_prefix("shared.") {
            if !["remote", "branch", "member"].contains(&name) {
                return Err(invalid(key, "unknown shared key"));
            }
            let map = encoded
                .get_mut("shared")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| invalid(key, "set the whole shared block first"))?;
            map.insert(name.into(), value);
            return Self::try_from(encoded);
        }
        if key == "precedents.half_life_days" {
            let root = encoded
                .as_object_mut()
                .ok_or_else(|| invalid("config", "expected a mapping"))?;
            let precedents = root
                .entry("precedents")
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()
                .ok_or_else(|| invalid("precedents", "expected a mapping of half_life_days"))?;
            if value.is_null() {
                precedents.remove("half_life_days");
            } else {
                precedents.insert("half_life_days".into(), value);
            }
            return Self::try_from(encoded);
        }
        if key == "open_weights" || key.starts_with("open_weights.") {
            set_open_weights(&mut encoded, key, value)?;
            return Self::try_from(encoded);
        }
        let target = match key.split_once('.') {
            // Omitted while unset, so the leaf is inserted before it is set.
            Some(("models", name)) if MODEL_OPTIONAL.contains(&name) => encoded
                .get_mut("models")
                .and_then(Value::as_object_mut)
                .map(|map| map.entry(name).or_insert(Value::Null)),
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
            Some(("brief", "task_workspaces")) => encoded
                .get_mut("brief")
                .and_then(Value::as_object_mut)
                .and_then(|map| map.get_mut("task_workspaces")),
            // Omitted while unset, so the section and leaf are inserted first.
            Some(("meetings", "agent_workspace")) => encoded
                .as_object_mut()
                .and_then(|root| {
                    root.entry("meetings")
                        .or_insert_with(|| Value::Object(Map::new()))
                        .as_object_mut()
                })
                .map(|map| map.entry("agent_workspace").or_insert(Value::Null)),
            // Omitted while `auto`, so the leaf is inserted before it is set.
            Some(("brief", "handoff")) => encoded
                .get_mut("brief")
                .and_then(Value::as_object_mut)
                .map(|map| map.entry("handoff").or_insert(Value::Null)),
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
