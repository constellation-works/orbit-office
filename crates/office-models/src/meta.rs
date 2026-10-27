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
//! a verbatim transcript through the same confirmed shape and for speaker
//! turns as JSON in the reply text. A reply that is not that JSON stays
//! text only.

use base64::Engine;
use office_core::ports::TranscriptSegment;
use serde_json::{Value, json};

/// Chat Completions URL for a provider base URL.
pub(crate) fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Instruction that asks for the verbatim transcript and speaker turns.
pub(crate) fn transcribe_instruction(language: Option<&str>) -> String {
    let hint = language.map_or(String::new(), |name| {
        format!(" The spoken language is {name}.")
    });
    format!(
        "Transcribe this audio verbatim.{hint} Reply with JSON only, shaped as \
         {{\"text\":\"<full transcript>\",\"segments\":[{{\"speaker\":\"<name or null>\",\
         \"start_ms\":<milliseconds from the start of this audio>,\"end_ms\":<milliseconds>,\
         \"text\":\"<speaker turn>\"}}]}}. Use one segment per speaker turn. \
         When you cannot report turns and timestamps, return an empty segments array."
    )
}

/// Body for a transcription call: the audio as one `input_audio` part in
/// `format`, which is `wav` or `mp3`, the only values the schema accepts.
pub(crate) fn transcribe_body(
    model: &str,
    format: &str,
    audio: &[u8],
    language: Option<&str>,
) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": transcribe_instruction(language)},
                {"type": "input_audio", "input_audio": {
                    "data": base64::engine::general_purpose::STANDARD.encode(audio),
                    "format": format,
                }},
            ],
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "transcript",
                "strict": true,
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "text": {"type": "string"},
                        "segments": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "speaker": {"type": ["string", "null"]},
                                    "start_ms": {"type": "integer", "minimum": 0},
                                    "end_ms": {"type": "integer", "minimum": 0},
                                    "text": {"type": "string"}
                                },
                                "required": ["speaker", "start_ms", "end_ms", "text"]
                            }
                        }
                    },
                    "required": ["text", "segments"]
                }
            }
        },
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

/// Transcript text and any speaker turns parsed from a Chat Completions reply.
pub(crate) struct ParsedTranscript {
    /// Full transcript text.
    pub text: String,
    /// Speaker turns, chunk-relative, when the reply reports them.
    pub segments: Vec<TranscriptSegment>,
}

/// Read a transcription reply.
///
/// When `choices[0].message.content` is the requested JSON object, `text`
/// and the valid `segments` are returned. A plain-text reply, including
/// one the model wrapped only in prose, is kept as the transcript with no
/// segments. A segment missing timestamps or text is dropped; its siblings
/// are kept.
pub(crate) fn parse_transcript(
    host: &str,
    response: &Value,
) -> Result<ParsedTranscript, office_core::Error> {
    let content = reply_text(host, response)?;
    Ok(parse_transcript_content(&content))
}

fn parse_transcript_content(content: &str) -> ParsedTranscript {
    let candidate = fenced_json(content).unwrap_or(content);
    parse_structured(candidate).unwrap_or(ParsedTranscript {
        text: content.to_string(),
        segments: Vec::new(),
    })
}

/// Body of one markdown code fence, when the whole reply is that fence.
fn fenced_json(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    let rest = trimmed.strip_prefix("```")?;
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let body = rest.trim_end().strip_suffix("```")?.trim();
    if body.is_empty() { None } else { Some(body) }
}

fn parse_structured(candidate: &str) -> Option<ParsedTranscript> {
    let value: Value = serde_json::from_str(candidate).ok()?;
    let object = value.as_object()?;
    let text = object.get("text")?.as_str()?.to_string();
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_segment).collect())
        .unwrap_or_default();
    Some(ParsedTranscript { text, segments })
}

fn parse_segment(value: &Value) -> Option<TranscriptSegment> {
    let object = value.as_object()?;
    let start_ms = object.get("start_ms")?.as_u64()?;
    let end_ms = object.get("end_ms")?.as_u64()?;
    if end_ms < start_ms {
        return None;
    }
    let text = object.get("text")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let speaker = match object.get("speaker") {
        None => None,
        Some(value) => speaker_name(value)?,
    };
    Some(TranscriptSegment {
        speaker,
        start_ms,
        end_ms,
        text: text.to_string(),
    })
}

/// `None` when `value` is not a speaker name or JSON null.
fn speaker_name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Null => Some(None),
        Value::String(name) => {
            let name = name.trim();
            if name.is_empty() {
                Some(None)
            } else {
                Some(Some(name.to_string()))
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
    }
}
