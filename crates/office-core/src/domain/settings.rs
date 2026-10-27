//! Typed Library settings. Parsing is pure; the adapter reads `.office/config.yaml`.
//!
//! The supported YAML subset is a two-level mapping, scalar values, and inline
//! or block string lists. Unsupported YAML constructs are refused, never guessed.

use std::collections::{BTreeMap, BTreeSet};

use office_common::Sensitivity;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

const KINDS: &[&str] = &[
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
    /// Days to retain each kind's original; `None` means keep indefinitely.
    pub retention: BTreeMap<String, Option<u32>>,
    /// Default sensitivity for each source kind.
    pub sensitivity: BTreeMap<String, Sensitivity>,
    /// Enabled packs (`meetings`, `receipts`).
    pub packs: Vec<String>,
    /// Signal policy thresholds.
    pub thresholds: SignalThresholds,
    /// Optional endpoint for the open-weight model; absent means unconfigured.
    pub open_weights_endpoint: Option<String>,
}

/// Positive, bounded signal thresholds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignalThresholds {
    /// Completed similar tasks needed for a repetition signal.
    pub repetition_count: u32,
    /// Repetition window in days.
    pub window_days: u32,
    /// Age in days at which knowledge needs verification.
    pub stale_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            retention: KINDS
                .iter()
                .map(|kind| {
                    (
                        (*kind).into(),
                        if *kind == "meeting" { Some(90) } else { None },
                    )
                })
                .collect(),
            sensitivity: KINDS
                .iter()
                .map(|kind| ((*kind).into(), Sensitivity::Normal))
                .collect(),
            packs: vec!["meetings".into()],
            thresholds: SignalThresholds {
                repetition_count: 3,
                window_days: 30,
                stale_days: 90,
            },
            open_weights_endpoint: None,
        }
    }
}

impl Settings {
    /// Parse the documented YAML subset (JSON mappings are also valid YAML).
    pub fn from_yaml(text: &str) -> Result<Self> {
        Self::try_from(parse_yaml(text)?)
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
                            .filter(|p| ["meetings", "receipts"].contains(p))
                            .ok_or_else(|| invalid(key, "expected meetings or receipts"))?;
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
                            _ => return Err(invalid(&dotted, "unknown threshold")),
                        }
                    }
                }
                "open_weights_endpoint" => {
                    settings.open_weights_endpoint = if value.is_null() {
                        None
                    } else {
                        let endpoint = value
                            .as_str()
                            .ok_or_else(|| invalid(key, "expected an HTTP(S) URL or null"))?;
                        let rest = endpoint
                            .strip_prefix("https://")
                            .or_else(|| endpoint.strip_prefix("http://"));
                        let host = rest.and_then(|v| v.split('/').next()).unwrap_or_default();
                        if host.is_empty()
                            || host.starts_with(':')
                            || host.contains('@')
                            || endpoint.chars().any(char::is_whitespace)
                        {
                            return Err(invalid(
                                key,
                                "expected an HTTP(S) URL with a host and no credentials",
                            ));
                        }
                        Some(endpoint.into())
                    };
                }
                _ => return Err(invalid(key, "unknown setting")),
            }
        }
        Ok(settings)
    }
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
        let line = raw.split_once('\r').map_or(raw, |(line, _)| line);
        let mut end = line.len();
        for (i, c) in line.char_indices() {
            if (c == '\'' || c == '"') && (quote.is_none() || quote == Some(c)) {
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
