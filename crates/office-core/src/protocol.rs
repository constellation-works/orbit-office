//! Orbit exec protocol: one request envelope in, one response envelope out.
//!
//! The request is `{"schema_version": 1, "tool": "<ns>.<verb>", "input": {...},
//! "context": {...}}`; the response is `{"ok": true, "output": {...}}` or
//! `{"ok": false, "error": {"code", "message", "retryable", "detail"?}}`.
//! Identity and secrets come from `context`, never from `input`. Unknown
//! input keys are refused, never silently dropped (STD-01 R29); that check
//! runs in `office-tools`, which knows each tool's accepted keys.

use serde_json::{Map, Value, json};

use crate::error::{Error, Result};

/// The exec protocol version this backend speaks.
pub const SCHEMA_VERSION: u64 = 1;

/// One parsed request envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The full tool name, `office.<verb>` for this backend.
    pub tool: String,
    /// Tool input (an empty object when absent).
    pub input: Map<String, Value>,
    /// Call context from Orbit: identity, secrets, roots (an empty object when absent).
    pub context: Map<String, Value>,
}

/// Parse a raw request envelope.
///
/// ```
/// let request =
///     office_core::protocol::parse(r#"{"schema_version": 1, "tool": "office.status"}"#)?;
/// assert_eq!(request.tool, "office.status");
/// assert!(request.input.is_empty());
/// # Ok::<(), office_core::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::InvalidRequest`] when the envelope is not a JSON object or its
/// `context` is not an object, [`Error::UnsupportedVersion`] for any other
/// `schema_version`, and [`Error::InvalidArgument`] when `input` is not an
/// object.
pub fn parse(raw: &str) -> Result<Request> {
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|error| Error::InvalidRequest(format!("request is not JSON: {error}")))?;
    let Value::Object(mut request) = parsed else {
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
    let tool = request
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let input = match request.remove("input") {
        None => Map::new(),
        Some(Value::Object(input)) => input,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "input must be a JSON object".to_string(),
            ));
        }
    };
    let context = match request.remove("context") {
        None => Map::new(),
        Some(Value::Object(context)) => context,
        Some(_) => {
            return Err(Error::InvalidRequest(
                "context must be a JSON object".to_string(),
            ));
        }
    };
    Ok(Request {
        tool,
        input,
        context,
    })
}

/// Render the one response envelope for a tool's outcome.
#[must_use]
pub fn respond(outcome: Result<Value>) -> String {
    match outcome {
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
