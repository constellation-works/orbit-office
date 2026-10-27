//! Helpers for exec request and response envelopes.

/// Build a request envelope with the supplied tool, input and context.
#[must_use]
pub fn request(tool: &str, input: serde_json::Value, context: serde_json::Value) -> String {
    serde_json::json!({"schema_version": 1, "tool": tool, "input": input, "context": context})
        .to_string()
}

/// Parse a response envelope.
///
/// # Errors
/// Returns the JSON parsing error if the response is malformed.
pub fn response(raw: &str) -> serde_json::Result<serde_json::Value> {
    serde_json::from_str(raw)
}
