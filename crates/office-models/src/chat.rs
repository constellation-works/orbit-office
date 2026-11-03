//! Request shapes: OpenAI-compatible Chat Completions (extraction, vision
//! and private or non-Meta transcription) on any configured endpoint, and
//! Meta's Muse Voice Transcribe one-shot endpoint (specs/models-and-privacy.md).
//!
//! Extraction and vision use OpenAI-compatible Chat Completions at the
//! endpoint's base URL (Meta's `https://api.meta.ai/v1` by default) with a
//! Bearer credential: one
//! `POST /chat/completions` whose reply text is `choices[0].message.content`,
//! with images as a base64 `image_url` data URL. Hosted extraction sends its
//! JSON Schema with `strict: false`, because the API's strict mode refuses
//! any schema with an optional property; the reply is validated locally
//! either way.
//!
//! Hosted transcription uses Muse Voice Transcribe's one-shot endpoint,
//! `POST /asr/transcribe` (Meta's cookbook, `06_muse_voice`, fetched
//! 2026-11-02): a multipart body with a `request` JSON part (`model`, `mode`,
//! `audioEncoding: "WAV"`, optional `languageBias`) and an `audio` part
//! holding the WAV. The reply is `{"sessionId","transcript",
//! "audioDurationMs","turns"}`, each turn carrying `startMs`, `transcript`
//! and, in `DIARIZATION` mode, `speaker`. The private route keeps the
//! OpenAI-compatible chat shape: audio as a base64 `input_audio` part, and
//! speaker turns asked for as JSON in the reply text. A reply that is not
//! that JSON stays text only.

use base64::Engine;
use office_core::ports::TranscriptSegment;
use serde_json::{Value, json};

/// Chat Completions URL for a provider base URL.
pub(crate) fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Muse Voice Transcribe one-shot URL for a provider base URL.
pub(crate) fn asr_url(base_url: &str) -> String {
    format!("{}/asr/transcribe", base_url.trim_end_matches('/'))
}

/// Multipart boundary for [`asr_body`]; never occurs in the JSON part, and
/// a WAV payload is delimited by the part's own CRLF and dashes.
const ASR_BOUNDARY: &str = "office-muse-voice-7f4507a1c9e3";

/// Content type and multipart body for one hosted transcription of `wav`.
/// `DIARIZATION` asks the model to label who spoke each turn.
pub(crate) fn asr_body(model: &str, wav: &[u8], language: Option<&str>) -> (String, Vec<u8>) {
    let mut request = json!({"model": model, "mode": "DIARIZATION", "audioEncoding": "WAV"});
    if let Some(language) = language {
        request["languageBias"] = json!([language]);
    }
    let mut body = format!(
        "--{ASR_BOUNDARY}\r\nContent-Disposition: form-data; name=\"request\"\r\n\
         Content-Type: application/json\r\n\r\n{request}\r\n\
         --{ASR_BOUNDARY}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"audio.wav\"\r\n\
         Content-Type: audio/wav\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{ASR_BOUNDARY}--\r\n").as_bytes());
    (
        format!("multipart/form-data; boundary={ASR_BOUNDARY}"),
        body,
    )
}

/// Read a Muse Voice Transcribe reply: `transcript` is the text, and each
/// turn with a start and text becomes a segment. A turn without `endMs`
/// ends where the next turn starts, or at the end of the audio.
pub(crate) fn parse_asr(
    host: &str,
    response: &Value,
) -> Result<ParsedTranscript, office_core::Error> {
    let text = response
        .get("transcript")
        .and_then(Value::as_str)
        .ok_or_else(|| office_core::Error::Model {
            code: "model_invalid_response",
            message: format!("model endpoint {host} returned no transcript"),
            retryable: false,
        })?
        .to_string();
    let turns: Vec<&Value> = response
        .get("turns")
        .and_then(Value::as_array)
        .map(|turns| turns.iter().collect())
        .unwrap_or_default();
    let audio_end = response.get("audioDurationMs").and_then(Value::as_u64);
    let mut segments = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        let Some(start_ms) = turn.get("startMs").and_then(Value::as_u64) else {
            continue;
        };
        let next_start = turns
            .get(index + 1)
            .and_then(|next| next.get("startMs"))
            .and_then(Value::as_u64);
        let end_ms = turn
            .get("endMs")
            .and_then(Value::as_u64)
            .or(next_start)
            .or(audio_end)
            .unwrap_or(start_ms)
            .max(start_ms);
        let Some(turn_text) = turn
            .get("transcript")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let speaker = match turn.get("speaker") {
            None => None,
            Some(value) => speaker_name(value).flatten(),
        };
        segments.push(TranscriptSegment {
            speaker,
            start_ms,
            end_ms,
            text: turn_text.to_string(),
        });
    }
    Ok(ParsedTranscript { text, segments })
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
/// The schema is not `strict`: the Meta Model API's strict mode refuses any
/// schema whose `required` omits a property (HTTP 400, seen live on
/// 2026-11-02), and `Models::extract` validates the reply locally anyway.
pub(crate) fn extract_body(model: &str, text: &str, schema: &Value) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": format!("Extract structured data from the following text. Reply with JSON only.\n\n{text}"),
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "extraction", "schema": schema, "strict": false},
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
