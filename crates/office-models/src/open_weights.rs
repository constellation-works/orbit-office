//! Request shapes for Muse Spark Open on the private route, the
//! open-weights model used for `private` sources, on whichever
//! OpenAI-compatible endpoint the user chose (specs/models-and-privacy.md).
//!
//! Every endpoint kind (a company's vLLM or SGLang deployment, a hosted
//! provider such as Baseten, or the user's own MLX hardware) serves Chat
//! Completions at `<endpoint>/chat/completions` with an optional Bearer
//! credential and the same `choices[0].message.content` reply (see
//! `lib.rs`). One documented divergence matters here: Muse Spark Open
//! accepts `json_schema` but does not enforce it, so extraction through
//! this module never trusts the reply shape: the caller parses the reply
//! as JSON and validates it locally against the requested schema, refusing
//! non-JSON and schema-violating replies (see `lib.rs`).

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
