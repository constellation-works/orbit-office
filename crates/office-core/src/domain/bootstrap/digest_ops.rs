//! Hashing, digest validation, routing and pricing for bootstrap plans.
use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::types::{Estimate, PHASES, Plan, Route, Snapshot};
use crate::Result;
use crate::domain::endpoint::Capability;
use crate::domain::metrics::metadata;
use crate::domain::settings::Settings;
use crate::domain::source::{sha256_hex, source_body, video};

pub(super) fn hash(value: &Value) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(value).map_err(|_| {
        video::refusal("invalid_bootstrap_data", "cannot encode bootstrap state")
    })?))
}
pub(super) fn signed(plan: &Plan) -> Result<String> {
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

pub(super) fn route(settings: &Settings, private: bool, careful: bool) -> Result<Route> {
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
            label: if settings.models.hosted_is_meta() {
                "Meta hosted API".into()
            } else {
                format!(
                    "Hosted: {}",
                    crate::domain::endpoint::host_of(settings.models.hosted_endpoint())
                        .unwrap_or_default()
                )
            },
            model: if careful {
                settings.models.careful.as_str()
            } else {
                settings.models.standard_model()
            }
            .into(),
        })
    }
}

pub(super) fn marked(markdown: &str) -> Result<String> {
    let (header, body) = crate::domain::knowledge::frontmatter::split(markdown)?;
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

pub(super) fn input_digest(
    snapshot: &Snapshot,
    plan: &Plan,
    settings: &Settings,
) -> Result<String> {
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
        // Session-brief workspaces do not affect bootstrap inference either.
        map.remove("brief");
    }
    if settings.demo.simulated_until.is_none()
        && let Some(settings) = settings_value.as_object_mut()
    {
        settings.remove("demo");
    }
    if settings.models == crate::domain::settings::ModelSettings::default()
        && let Some(map) = settings_value.as_object_mut()
    {
        map.remove("models");
    }
    hash(&json!({"sources":inputs,"settings":settings_value,"ids":plan.sources}))
}
pub(super) fn evidence_digest(snapshot: &Snapshot) -> Result<String> {
    hash(
        &json!({"digests": snapshot.sources.iter().map(|s| json!({"path":s.path,"digest":s.digest})).collect::<Vec<_>>(),"pages":snapshot.pages}),
    )
}

pub(super) fn price(
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
