use std::collections::BTreeSet;

use office_common::Sensitivity;
use serde_json::Value;

use super::helpers::{invalid, open_weights, positive, simulated_until, workspace_selector};
use super::model::{model_slug, number};
use super::types::{HandoffMode, KINDS, PrecedentSettings, RETIRED_KINDS, Settings};
use crate::{Error, Result};

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
                        if RETIRED_KINDS.contains(&kind.as_str()) {
                            continue;
                        }
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
                        match name.as_str() {
                            "careful" => settings.models.careful = model_slug(value, &key)?,
                            "standard" | "transcribe" => {
                                let slug = if value.is_null() {
                                    None
                                } else {
                                    Some(model_slug(value, &key)?)
                                };
                                if name == "standard" {
                                    settings.models.standard = slug;
                                } else {
                                    settings.models.transcribe = slug;
                                }
                            }
                            "endpoint" => {
                                settings.models.endpoint = if value.is_null() {
                                    None
                                } else {
                                    let url = value.as_str().ok_or_else(|| {
                                        invalid(&key, "expected a base URL or null")
                                    })?;
                                    crate::domain::endpoint::allow_hosted_endpoint(url)
                                        .map_err(|reason| invalid(&key, reason))?;
                                    Some(url.into())
                                }
                            }
                            _ => return Err(invalid(&key, "unknown model tier")),
                        }
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
                "brief" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let dotted = format!("brief.{name}");
                        if name == "handoff" {
                            settings.brief.handoff = match value.as_str() {
                                Some("auto") => HandoffMode::Auto,
                                Some("manual") => HandoffMode::Manual,
                                _ => return Err(invalid(&dotted, "expected auto or manual")),
                            };
                            continue;
                        }
                        if name != "task_workspaces" {
                            return Err(invalid(&dotted, "unknown brief setting"));
                        }
                        let list = value.as_array().ok_or_else(|| {
                            invalid(&dotted, "expected a list of workspace selectors")
                        })?;
                        if list.len() > 16 {
                            return Err(invalid(&dotted, "at most 16 workspace selectors"));
                        }
                        let mut seen = BTreeSet::new();
                        settings.brief.task_workspaces.clear();
                        for item in list {
                            let selector = workspace_selector(item, &dotted)?;
                            if !seen.insert(selector) {
                                return Err(invalid(&dotted, "duplicate workspace selector"));
                            }
                            settings.brief.task_workspaces.push(selector.into());
                        }
                    }
                }
                "meetings" => {
                    let items = value
                        .as_object()
                        .ok_or_else(|| invalid(key, "expected a mapping"))?;
                    for (name, value) in items {
                        let dotted = format!("meetings.{name}");
                        match name.as_str() {
                            "agent_workspace" => {
                                settings.meetings.agent_workspace = if value.is_null() {
                                    None
                                } else {
                                    Some(workspace_selector(value, &dotted)?.into())
                                }
                            }
                            _ => return Err(invalid(&dotted, "unknown meetings setting")),
                        }
                    }
                }
                "precedents" => settings.precedents = PrecedentSettings::parse(value)?,
                // A `scribe:` block written by an earlier build (the scribe
                // moved to the standalone scribe plugin, 4_decisions.md) is
                // accepted and ignored, so an existing Library still loads.
                // It is not carried forward when office.config-set rewrites
                // the file; the Library's git history keeps it.
                "scribe" => {}
                "shared" => settings.shared = crate::domain::sharing::SharedSettings::parse(value)?,
                "calendar" => {
                    settings.calendar = crate::domain::calendar::CalendarSettings::parse(value)?;
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
