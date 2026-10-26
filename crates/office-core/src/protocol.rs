//! Orbit exec protocol: one request envelope in, one response envelope out.
//!
//! The request is `{"schema_version": 1, "tool": "<ns>.<verb>", "input": {...},
//! "context": {...}}`; the response is `{"ok": true, "output": {...}}` or
//! `{"ok": false, "error": {"code", "message", "retryable", "detail"?}}`.
//! Identity and secrets come from `context`, never from `input`. Unknown
//! input keys are refused, never silently dropped (STD-01 R29).

use serde_json::{Map, Value, json};

use crate::NAMESPACE;
use crate::error::{Error, Result};
use crate::tools;

/// The exec protocol version this backend speaks.
pub const SCHEMA_VERSION: u64 = 1;

type Handler = fn(&Map<String, Value>, &Map<String, Value>) -> Value;

/// Every verb this backend serves: name, accepted input keys, handler.
/// The manifest, schemas and goldens are held equal to this list by tests.
pub fn tool_table() -> Vec<(&'static str, Vec<&'static str>, Handler)> {
    vec![("status", Vec::new(), tools::status as Handler)]
}

/// Answer one raw request with one serialized response envelope.
pub fn serve(raw: &str) -> String {
    match parse(raw).and_then(dispatch) {
        Ok(output) => json!({ "ok": true, "output": output }).to_string(),
        Err(error) => {
            let body = json!({
                "code": error.code(),
                "message": error.to_string(),
                "retryable": error.retryable(),
            });
            json!({ "ok": false, "error": body }).to_string()
        }
    }
}

fn dispatch(request: Map<String, Value>) -> Result<Value> {
    let tool = request.get("tool").and_then(Value::as_str).unwrap_or("");
    let prefix = format!("{NAMESPACE}.");
    let Some(verb) = tool.strip_prefix(&prefix) else {
        return Err(Error::UnknownTool(format!(
            "{tool:?} (this backend serves only {NAMESPACE}.* tools)"
        )));
    };
    let Some((_, accepted, handler)) = tool_table().into_iter().find(|(name, _, _)| *name == verb)
    else {
        return Err(Error::UnknownTool(format!(
            "{tool} is not a {NAMESPACE} tool"
        )));
    };

    let input_value = request
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let Some(input) = input_value.as_object() else {
        return Err(Error::InvalidArgument(
            "input must be a JSON object".to_string(),
        ));
    };
    let mut unknown: Vec<&str> = input
        .keys()
        .map(String::as_str)
        .filter(|key| !accepted.contains(key))
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(Error::InvalidArgument(format!(
            "{tool} does not accept: {}",
            unknown.join(", ")
        )));
    }

    let context_value = request
        .get("context")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let Some(context) = context_value.as_object() else {
        return Err(Error::InvalidRequest(
            "context must be a JSON object".to_string(),
        ));
    };
    Ok(handler(input, context))
}

fn parse(raw: &str) -> Result<Map<String, Value>> {
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|error| Error::InvalidRequest(format!("request is not JSON: {error}")))?;
    let Some(request) = parsed.as_object() else {
        return Err(Error::InvalidRequest(
            "request must be a JSON object".to_string(),
        ));
    };
    let version = request.get("schema_version").and_then(Value::as_u64);
    if version != Some(SCHEMA_VERSION) {
        return Err(Error::UnsupportedVersion(format!(
            "this backend speaks exec schema_version {SCHEMA_VERSION}, got {:?}",
            request.get("schema_version")
        )));
    }
    Ok(request.clone())
}
