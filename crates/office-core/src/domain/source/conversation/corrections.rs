//! Append-only corrections and explicit user additions. Stored JSON payloads
//! fit inside one log line; projecting them never changes the source bytes.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::session::{Entry, entries};
use crate::{Error, Result};

/// The user's correction choice, independent of the resulting entry kind.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Reclassify as open.
    NotDecision,
    /// Replace the entry kind.
    Kind,
    /// Replace the wording, retaining the effective kind.
    Wording,
    /// Omit from derived readers.
    Remove,
}

impl Choice {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotDecision => "not_decision",
            Self::Kind => "kind",
            Self::Wording => "wording",
            Self::Remove => "remove",
        }
    }
}

/// Corrections use the existing-source append path; creation cannot bypass
/// anchor checks. Shared by the production adapter and its test double.
pub fn validate_capture(request: &crate::ports::CaptureSourceRequest) -> Result<()> {
    if request.append_to.is_none() && request.entries.iter().any(|e| e.kind == "correction") {
        return Err(Error::InvalidArgument(
            "correction entries require append_to".into(),
        ));
    }
    Ok(())
}

/// One stored correction, naming an original entry in this source.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Correction {
    /// Local body-relative anchor, such as #L1.
    pub anchor: String,
    /// The requested change.
    pub choice: Choice,
    /// Required only for a kind change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Required only for a wording change, copied verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

fn invalid(message: &str) -> Error {
    Error::Library {
        code: "invalid_conversation_correction",
        message: message.into(),
    }
}

impl Correction {
    /// Decode and validate a canonical correction payload.
    pub fn parse(text: &str) -> Result<Self> {
        let correction: Self = serde_json::from_str(text).map_err(|_| {
            invalid("correction needs anchor, choice and only that choice's fields")
        })?;
        correction.line()?;
        let valid = match correction.choice {
            Choice::NotDecision | Choice::Remove => {
                correction.kind.is_none() && correction.text.is_none()
            }
            Choice::Kind => {
                correction.text.is_none()
                    && correction.kind.as_deref().is_some_and(|kind| {
                        super::super::is_conversation_kind(kind) && kind != "correction"
                    })
            }
            Choice::Wording => {
                correction.kind.is_none()
                    && correction
                        .text
                        .as_deref()
                        .is_some_and(|text| !text.trim().is_empty())
            }
        };
        if !valid {
            return Err(invalid(
                "not_decision/remove take no extra fields; kind needs a non-correction kind; wording needs nonempty text",
            ));
        }
        if correction
            .text
            .as_ref()
            .is_some_and(|text| super::scrub(text) != *text)
        {
            return Err(invalid(
                "correction wording contains credentials; remove them before appending",
            ));
        }
        Ok(correction)
    }

    /// Checked body-relative target line.
    pub fn line(&self) -> Result<usize> {
        let line = self
            .anchor
            .strip_prefix("#L")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|line| *line > 0 && self.anchor == format!("#L{line}"));
        line.ok_or_else(|| invalid("correction anchor must be a local #L<n> entry anchor"))
    }
}

/// Validate explicit user attribution and its user-chosen entry kind.
/// Attribution records a caller claim, never authorization or identity.
pub fn validate_user_text(kind: &str, text: &str, by: &str) -> Result<()> {
    if by != "user" || !["direction", "decision", "fact"].contains(&kind) || text.trim().is_empty()
    {
        return Err(Error::InvalidArgument(
            "by must be user and requires a nonempty direction, decision or fact".into(),
        ));
    }
    if super::scrub(text) != text {
        return Err(invalid(
            "user entry contains credentials; remove them before appending",
        ));
    }
    Ok(())
}

/// Parse an object at the capture boundary. Aliases retain the capture contract.
pub fn capture_entry(value: &Value) -> Result<crate::ports::ConversationEntry> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(alias = "type")]
        kind: String,
        #[serde(default, alias = "message", alias = "content")]
        text: Option<String>,
        by: Option<String>,
        correction: Option<Value>,
        scope: Option<String>,
        until: Option<String>,
    }
    let input: Input = serde_json::from_value(value.clone()).map_err(|_| {
        Error::InvalidArgument(
            "capture entry needs kind and text (or correction), with no unknown fields".into(),
        )
    })?;
    let text = if let Some(correction) = input.correction {
        if input.kind != "correction" || input.text.is_some() || input.by.is_some() {
            return Err(invalid(
                "correction payload requires kind correction and cannot include text or by",
            ));
        }
        let text = correction.to_string();
        Correction::parse(&text)?;
        text
    } else {
        let text = input
            .text
            .ok_or_else(|| Error::InvalidArgument("capture entry needs text".into()))?;
        if let Some(by) = input.by.as_deref() {
            validate_user_text(&input.kind, &text, by)?;
        }
        text
    };
    // Scope is the writer's explicit declaration, never inferred from text.
    let global = match (input.scope.as_deref(), input.until) {
        (None | Some("project"), None) => None,
        (Some("global"), until) if input.kind == "handoff" => {
            Some(crate::domain::session::GlobalScope { until })
        }
        (Some("global"), _) => {
            return Err(Error::InvalidArgument(
                "scope: global applies only to handoff entries".into(),
            ));
        }
        (None | Some("project"), Some(_)) => {
            return Err(Error::InvalidArgument(
                "until applies only to a handoff with scope: global".into(),
            ));
        }
        (Some(_), _) => {
            return Err(Error::InvalidArgument(
                "capture entry scope must be global or project".into(),
            ));
        }
    };
    Ok(crate::ports::ConversationEntry {
        kind: input.kind,
        text,
        by: input.by,
        global,
    })
}

/// Effective log, with original anchors and times. Corrections compose in
/// append order; removal remains final. Correction records are never evidence
/// entries themselves. Invalid targets refuse the whole projection.
pub fn effective_entries(body: &str) -> Result<Vec<Entry>> {
    let log = entries(body)?;
    let mut result: Vec<Entry> = Vec::new();
    let mut removed = std::collections::BTreeSet::new();
    for entry in log {
        if entry.kind != "correction" {
            result.push(entry);
            continue;
        }
        let correction = Correction::parse(&entry.text)?;
        let line = correction.line()?;
        let target = result.iter_mut().find(|e| e.line == line).ok_or_else(|| {
            invalid(&format!(
                "correction target {} is not an earlier original entry",
                correction.anchor
            ))
        })?;
        match correction.choice {
            Choice::NotDecision => target.kind = "open".into(),
            Choice::Kind => {
                if let Some(kind) = correction.kind {
                    target.kind = kind;
                }
            }
            Choice::Wording => {
                if let Some(text) = correction.text {
                    target.text = text;
                }
            }
            Choice::Remove => {
                removed.insert(line);
            }
        }
        target.corrected = true;
    }
    result.retain(|e| !removed.contains(&e.line));
    Ok(result)
}

/// Validate every appended correction against entries already in the source,
/// before any write. The batch is all-or-nothing, including ordinary entries.
pub fn validate_append(body: &str, lines: &[String]) -> Result<()> {
    let original = entries(body)?;
    for line in lines {
        for entry in entries(line)? {
            if entry.kind == "correction" {
                let correction = Correction::parse(&entry.text)?;
                let target = correction.line()?;
                if !original
                    .iter()
                    .any(|e| e.line == target && e.kind != "correction")
                {
                    return Err(invalid(&format!(
                        "correction target {} does not name an original entry in this source",
                        correction.anchor
                    )));
                }
            }
        }
    }
    let separator = if body.is_empty() || body.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    effective_entries(&format!("{body}{separator}{}", lines.join("\n")))?;
    Ok(())
}
