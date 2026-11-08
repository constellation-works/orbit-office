//! Structured handoff validation, one-line storage and ordered rendering.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{metrics::timestamp, source::conversation::scrub};
use crate::{Error, Result};

const MARKER: &str = "(structured) ";

/// A validated handoff. Field order is the display contract, independent of
/// JSON map ordering. Authorization is a recorded claim, never authority.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Structured {
    objective: String,
    next: String,
    #[serde(default)]
    authorization: String,
    #[serde(default)]
    observations: Vec<Observation>,
    #[serde(default)]
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Observation {
    text: String,
    observed_at: String,
    recheck: String,
}

fn invalid(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

fn required(value: &Value, field: &str, code: &'static str) -> Result<()> {
    if value
        .get(field)
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err(invalid(
            code,
            format!("structured handoff {field} is required and must be nonempty text"),
        ));
    }
    Ok(())
}

fn budget(field: &str, values: &[&str], limit: usize) -> Result<()> {
    if values.iter().map(|s| s.chars().count()).sum::<usize>() > limit {
        return Err(invalid(
            "handoff_budget_exceeded",
            format!("structured handoff {field} exceeds its {limit}-character budget"),
        ));
    }
    for text in values {
        if text.chars().any(char::is_control) {
            return Err(invalid(
                "handoff_field_invalid",
                format!("structured handoff {field} must contain single-line text"),
            ));
        }
        if scrub(text) != *text {
            return Err(invalid(
                "handoff_field_invalid",
                format!(
                    "structured handoff {field} contains credentials; remove them before capture"
                ),
            ));
        }
        validate_task_ids(text)?;
    }
    Ok(())
}

/// Bare numeric references are ambiguous. Reject bare runs of four or more
/// digits, or any bare number after a task label; allow explicit task IDs,
/// links, dates, versions, anchors and quantities with units.
fn validate_task_ids(text: &str) -> Result<()> {
    let mut task_label = false;
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| "()[]{}<>,;.!?\"'".contains(c));
        if word.contains("://") || timestamp(word).is_some() {
            task_label = false;
            continue;
        }
        for token in word
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '#' && c != '.')
            .filter(|token| !token.is_empty())
        {
            if !token.is_empty()
                && token.bytes().all(|b| b.is_ascii_digit())
                && (token.len() >= 4 || task_label)
            {
                return Err(invalid(
                    "handoff_task_id_incomplete",
                    "structured handoff task references must use complete IDs such as ORB-<id>; bare numeric references are refused",
                ));
            }
            task_label =
                ["task", "tasks", "assignment"].contains(&token.to_ascii_lowercase().as_str());
        }
    }
    Ok(())
}

impl Structured {
    /// Decode at the capture boundary, naming missing fields before serde's
    /// structural checks. No submitted content is copied into errors.
    pub fn from_value(value: &Value) -> Result<Self> {
        required(value, "objective", "handoff_required_field")?;
        required(value, "next", "handoff_required_field")?;
        if let Some(observations) = value.get("observations").and_then(Value::as_array) {
            for observation in observations {
                required(observation, "observed_at", "handoff_observed_at_required")?;
            }
        }
        let handoff: Self = serde_json::from_value(value.clone()).map_err(|_| invalid(
            "handoff_structure_invalid",
            "structured handoff accepts objective, next, authorization, observations [{text, observed_at, recheck}] and evidence [text], with no unknown fields",
        ))?;
        handoff.validate()?;
        Ok(handoff)
    }

    fn validate(&self) -> Result<()> {
        budget("objective", &[&self.objective], 500)?;
        budget("next", &[&self.next], 1000)?;
        budget("authorization", &[&self.authorization], 1000)?;
        let observation_text: Vec<&str> = self
            .observations
            .iter()
            .flat_map(|o| [o.text.as_str(), o.observed_at.as_str(), o.recheck.as_str()])
            .collect();
        budget("observations", &observation_text, 2000)?;
        let evidence: Vec<&str> = self.evidence.iter().map(String::as_str).collect();
        budget("evidence", &evidence, 2000)?;
        if super::cited_assignment(&self.objective).is_none() {
            return Err(invalid(
                "handoff_assignment_required",
                "structured handoff objective must cite the complete assignment task ID",
            ));
        }
        for observation in &self.observations {
            if timestamp(&observation.observed_at).is_none() {
                return Err(invalid(
                    "handoff_observed_at_invalid",
                    "structured handoff observed_at must be RFC 3339 with an explicit offset",
                ));
            }
            if observation.text.trim().is_empty() || observation.recheck.trim().is_empty() {
                return Err(invalid(
                    "handoff_required_field",
                    "structured handoff observations require nonempty text and recheck",
                ));
            }
        }
        if self.evidence.iter().any(|e| e.trim().is_empty()) {
            return Err(invalid(
                "handoff_required_field",
                "structured handoff evidence items must be nonempty links or references",
            ));
        }
        Ok(())
    }

    /// One physical log line, with a versionable marker distinct from prose.
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_string(self).map_err(|_| {
            invalid(
                "handoff_structure_invalid",
                "cannot encode structured handoff",
            )
        })?;
        Ok(format!("{MARKER}{json}"))
    }

    /// Plain text shared by the brief and `/handoff`, in the required order.
    #[must_use]
    pub fn render(&self) -> String {
        let authorization = if self.authorization.is_empty() {
            "Not recorded."
        } else {
            &self.authorization
        };
        let observations = self
            .observations
            .iter()
            .map(|o| {
                format!(
                    "- {} (observed at {}); recheck: {}",
                    o.text, o.observed_at, o.recheck
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let evidence = self
            .evidence
            .iter()
            .map(|e| format!("- {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Objective: {}\nNext: {}\nAuthorization: {}\nObservations:\n{}\nEvidence:\n{}",
            self.objective,
            self.next,
            authorization,
            if observations.is_empty() {
                "- None recorded."
            } else {
                &observations
            },
            if evidence.is_empty() {
                "- None recorded."
            } else {
                &evidence
            }
        )
    }
}

/// Decode a stored entry after its optional scope and scribe markers.
/// Ordinary historical text is preserved as legacy. Malformed structured
/// payloads refuse the read instead of appearing as a legacy note.
pub fn decode(text: &str) -> Result<Option<Structured>> {
    let text = if super::looks_global(text) {
        super::stored_scope(text)?;
        text.split_once(") ").map_or(text, |(_, tail)| tail)
    } else {
        text
    };
    let text = text.strip_prefix("(scribe) ").unwrap_or(text);
    let Some(json) = text.strip_prefix(MARKER) else {
        return Ok(None);
    };
    let value = serde_json::from_str(json).map_err(|_| {
        invalid(
            "handoff_structure_invalid",
            "stored structured handoff must be valid JSON",
        )
    })?;
    Structured::from_value(&value).map(Some)
}

/// Preserve scope and scribe attribution while rendering the structured body.
#[must_use]
pub fn render(text: &str, handoff: &Structured) -> String {
    let prefix = text.split_once(MARKER).map_or("", |(prefix, _)| prefix);
    format!("{prefix}{}", handoff.render())
}
