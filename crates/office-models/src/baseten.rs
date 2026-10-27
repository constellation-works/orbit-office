//! Baseten client shapes for Muse Spark Open, the open-weights model
//! used for `private` sources (specs/models-and-privacy.md).
//!
//! Confirmed against Baseten's published Model APIs docs on 2026-10-27
//! (see `lib.rs`): Chat Completions at
//! `https://inference.baseten.co/v1/chat/completions` (a dedicated
//! deployment answers under `https://model-<id>.api.baseten.co/...`),
//! Bearer credential, same `choices[0].message.content` reply. One
//! documented divergence matters here: Muse Spark Open accepts
//! `json_schema` but does not enforce it, so extraction through this
//! module never trusts the reply shape — the caller parses the reply as
//! JSON and refuses anything else (see `lib.rs`).

use serde_json::{Value, json};

/// Body for an open-weights extraction call. The schema travels with the
/// prompt as well as in `response_format`, because the endpoint does not
/// enforce `response_format` for this model.
pub(crate) fn extract_body(model: &str, text: &str, schema: &Value) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": format!(
                "Extract structured data from the following text. Reply with JSON only, matching this JSON Schema:\n{schema}\n\n{text}"
            ),
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "extraction", "schema": schema, "strict": true},
        },
    })
}
