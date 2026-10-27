//! Meta Model API request shapes: Muse Spark (extraction, vision) and
//! Muse Voice Transcribe (specs/models-and-privacy.md).
//!
//! Confirmed against Meta's published docs on 2026-10-27 (see the crate
//! documentation in `lib.rs` for the source list): the API is
//! OpenAI-compatible Chat Completions at `https://api.meta.ai/v1` with a
//! Bearer credential, so every hosted operation is one
//! `POST /chat/completions` whose reply text is
//! `choices[0].message.content`. Audio reaches the model as a base64
//! `input_audio` content part and images as a base64 `image_url` data
//! URL; no dedicated transcription endpoint shape was published in the
//! fetched references, so transcription asks the audio-capable model for
//! a verbatim transcript through the same confirmed shape.

use base64::Engine;
use serde_json::{Value, json};

/// Chat Completions URL for a provider base URL.
pub(crate) fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Body for a transcription call: the audio as one `input_audio` part.
pub(crate) fn transcribe_body(
    model: &str,
    mime: &str,
    audio: &[u8],
    language: Option<&str>,
) -> Value {
    let format = mime.rsplit('/').next().unwrap_or("wav");
    let hint = language.map_or(String::new(), |name| {
        format!(" The spoken language is {name}.")
    });
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": format!("Transcribe this audio verbatim.{hint}")},
                {"type": "input_audio", "input_audio": {
                    "data": base64::engine::general_purpose::STANDARD.encode(audio),
                    "format": format,
                }},
            ],
        }],
    })
}

/// Body for a structured extraction call, asking for `schema`-shaped JSON.
pub(crate) fn extract_body(model: &str, text: &str, schema: &Value) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": format!("Extract structured data from the following text. Reply with JSON only.\n\n{text}"),
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "extraction", "schema": schema, "strict": true},
        },
    })
}

/// Body for an image-reading call: the image as one `image_url` data URL.
pub(crate) fn image_body(model: &str, mime: &str, image: &[u8], prompt: &str) -> Value {
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(image)
    );
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": data_url}},
            ],
        }],
    })
}

/// The reply text of a Chat Completions response.
pub(crate) fn reply_text(host: &str, response: &Value) -> Result<String, office_core::Error> {
    response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| office_core::Error::Model {
            code: "model_invalid_response",
            message: format!("model endpoint {host} returned no message content"),
            retryable: false,
        })
}
