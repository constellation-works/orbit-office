//! Orbit's provider-neutral v1 wire request and bounds.
use crate::{Error, Result};
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use serde_json::Value;

pub(super) const CONTRACT: &str = "orbit.precedent-provider/1";
pub(super) const STRING_BYTES: usize = 8192;
pub(super) const RESPONSE_BYTES: usize = 262_144;

/// A validated host request. Office's provider caps `k` at eight.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    contract: String,
    pub(super) workspace_id: String,
    pub(super) request: Query,
    #[serde(deserialize_with = "timestamp")]
    pub(super) as_of: DateTime<FixedOffset>,
    pub(super) k: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Query {
    id: String,
    gate: String,
    pub(super) category: String,
    pub(super) question: String,
    pub(super) options: Vec<OptionRecord>,
    #[serde(rename = "topic")]
    _topic: Option<String>,
    #[serde(deserialize_with = "timestamp")]
    raised_at: DateTime<FixedOffset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OptionRecord {
    id: String,
    pub(super) label: String,
    #[serde(rename = "consequence")]
    _consequence: Option<String>,
}

fn timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<DateTime<FixedOffset>, D::Error> {
    let text = String::deserialize(deserializer)?;
    DateTime::parse_from_rfc3339(&text).map_err(|_| serde::de::Error::custom("expected RFC 3339"))
}

pub(super) fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(format!("decision-precedents {}", message.into()))
}

pub(super) fn id_valid(text: &str, bound: usize) -> bool {
    !text.trim().is_empty() && text.len() <= bound && !text.chars().any(char::is_control)
}

fn strings_bounded(value: &Value) -> bool {
    match value {
        Value::String(text) => text.len() <= STRING_BYTES,
        Value::Array(items) => items.iter().all(strings_bounded),
        Value::Object(map) => map
            .iter()
            .all(|(key, value)| key.len() <= STRING_BYTES && strings_bounded(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
    }
}

impl Request {
    /// Reject unknown fields, malformed times/options and contract bounds
    /// before accessing the Library. Error text never echoes the payload.
    pub fn parse(value: Value) -> Result<Self> {
        if value
            .get("contract")
            .and_then(Value::as_str)
            .is_some_and(|contract| contract != CONTRACT)
        {
            return Err(Error::Library {
                code: "precedent_contract_unsupported",
                message: format!("decision-precedents requires {CONTRACT}"),
            });
        }
        let bytes = serde_json::to_vec(&value).map_err(|_| invalid("request cannot be encoded"))?;
        if bytes.len() > 65_536 || !strings_bounded(&value) {
            return Err(invalid("request exceeds the contract byte bounds"));
        }
        let out: Self = serde_json::from_value(value).map_err(|_| invalid("requires the closed orbit.precedent-provider/1 request shape, with RFC 3339 timestamps"))?;
        if out.contract != CONTRACT || !(1..=8).contains(&out.k) {
            return Err(invalid(
                "contract must be orbit.precedent-provider/1 and k must be an integer from 1 to 8",
            ));
        }
        let query = &out.request;
        if ![&out.workspace_id, &query.id, &query.gate, &query.category]
            .iter()
            .all(|id| id_valid(id, 256))
            || query.question.trim().is_empty()
            || out.as_of != query.raised_at
        {
            return Err(invalid(
                "ids must be nonempty, control-free and at most 256 bytes; question must be nonempty; as_of must equal request.raised_at",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        if !(1..=8).contains(&query.options.len())
            || query.options.iter().any(|option| {
                !id_valid(&option.id, 64)
                    || option.label.trim().is_empty()
                    || !ids.insert(&option.id)
            })
        {
            return Err(invalid(
                "request.options must contain 1 to 8 nonempty labels with distinct ids of at most 64 bytes",
            ));
        }
        Ok(out)
    }

    /// Question and option labels used for lexical retrieval.
    #[must_use]
    pub fn lexical_query(&self) -> String {
        std::iter::once(self.request.question.as_str())
            .chain(self.request.options.iter().map(|o| o.label.as_str()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}
