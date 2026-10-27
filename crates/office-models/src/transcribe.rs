//! Stage only long recordings, then route every chunk through the same client.

use std::io::Write;

use office_core::domain::endpoint::Capability;
use office_core::ports::{Routed, TranscribeRequest, Transcript, TranscriptChunk};
use office_core::{Error, Result};

use crate::{RoutedClient, http, media, meta, routing};

/// A planned window always spends exactly one inference, independently of the
/// general audio splitter's configurable threshold.
pub(crate) fn planned(
    client: &RoutedClient,
    request: &TranscribeRequest,
) -> Result<Routed<Transcript>> {
    if request.duration_seconds == 0 || request.duration_seconds > 600 {
        return Err(office_core::domain::source::video::refusal(
            "plan_exceeded",
            "planned audio window exceeds ten minutes",
        ));
    }
    let format = audio_format(&request.mime)?;
    let resolved = routing::route(request.sensitivity, Capability::Audio, &client.config)?;
    let parsed = client.transcribe_chunk(
        request.sensitivity,
        format,
        &request.audio,
        request.language.as_deref(),
    )?;
    Ok(Routed {
        value: shift_turns(parsed, 0)?,
        derived_by: resolved.derived_by(),
    })
}

pub(crate) fn transcribe(
    client: &RoutedClient,
    request: &TranscribeRequest,
) -> Result<Routed<Transcript>> {
    // Refuse a container the model schema cannot take before any call or staging.
    let format = audio_format(&request.mime)?;
    // Refuse an unusable route before staging private media or starting
    // ffmpeg. Every chunk resolves the same route, so this is its record.
    let resolved = routing::route(request.sensitivity, Capability::Audio, &client.config)?;
    http::check_url(&meta::chat_url(&resolved.base_url), resolved.destination())?;
    let derived_by = resolved.derived_by();
    let call = |audio: &[u8], offset_ms: u64| {
        let parsed = client.transcribe_chunk(
            request.sensitivity,
            format,
            audio,
            request.language.as_deref(),
        )?;
        shift_turns(parsed, offset_ms)
    };
    let splitter = &client.config.audio_splitter;
    if !media::needs_split(request.duration_seconds, splitter.threshold_seconds) {
        return Ok(Routed {
            value: call(&request.audio, 0)?,
            derived_by,
        });
    }

    let directory = tempfile::tempdir().map_err(|error| media_io("create audio staging", error))?;
    let mut input = tempfile::Builder::new()
        .suffix(&format!(".{format}"))
        .tempfile_in(directory.path())
        .map_err(|error| media_io("create staged audio", error))?;
    input
        .write_all(&request.audio)
        .map_err(|error| media_io("write staged audio", error))?;
    let paths =
        splitter.split_if_needed(input.path(), directory.path(), request.duration_seconds)?;
    let plan = media::segment_plan(request.duration_seconds, splitter.max_segment_seconds);
    if plan.len() != paths.len() {
        return Err(Error::Model {
            code: "media_split_failed",
            message: "ffmpeg split count did not match the segment plan".into(),
            retryable: false,
        });
    }
    let mut texts = Vec::with_capacity(paths.len());
    let mut segments = Vec::new();
    let mut chunks = Vec::with_capacity(paths.len());
    for (path, piece) in paths.iter().zip(plan) {
        let audio = std::fs::read(path).map_err(|error| media_io("read split audio", error))?;
        let offset_ms = piece
            .start_seconds
            .checked_mul(1_000)
            .ok_or_else(timestamp_overflow)?;
        let chunk = call(&audio, offset_ms)?;
        // `shift_turns` already classified this reply: a text-only chunk
        // stays in `chunks` where the flat turn list would drop it.
        let Transcript {
            text,
            segments: chunk_segments,
            chunks: chunk_chunks,
        } = chunk;
        texts.push(text);
        segments.extend(chunk_segments);
        chunks.extend(chunk_chunks);
    }
    Ok(Routed {
        value: Transcript {
            text: texts.join("\n"),
            segments,
            chunks,
        },
        derived_by,
    })
}

fn timestamp_overflow() -> Error {
    Error::Model {
        code: "model_invalid_response",
        message: "transcript timestamps overflow when aligned to the recording".into(),
        retryable: false,
    }
}

fn shift_turns(mut parsed: meta::ParsedTranscript, offset_ms: u64) -> Result<Transcript> {
    for segment in &mut parsed.segments {
        segment.start_ms = segment
            .start_ms
            .checked_add(offset_ms)
            .ok_or_else(timestamp_overflow)?;
        segment.end_ms = segment
            .end_ms
            .checked_add(offset_ms)
            .ok_or_else(timestamp_overflow)?;
    }
    let chunks = if parsed.segments.is_empty() {
        vec![TranscriptChunk::Text(parsed.text.clone())]
    } else {
        vec![TranscriptChunk::Turns(parsed.segments.clone())]
    };
    Ok(Transcript {
        text: parsed.text,
        segments: parsed.segments,
        chunks,
    })
}

// The model schema takes only `wav` or `mp3` for `input_audio.format`, and
// ffmpeg picks the chunk container from the staged suffix. Both come from this
// one canonical value, so aliases are normalized and no other container is sent
// or relabelled. Caller-supplied MIME text never reaches a path or a request.
fn audio_format(mime: &str) -> Result<&'static str> {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "audio/wav" | "audio/wave" | "audio/x-wav" => Ok("wav"),
        "audio/mpeg" | "audio/mp3" => Ok("mp3"),
        _ => Err(Error::Model {
            code: "media_unsupported_format",
            message: "transcription needs WAV or MP3 audio (audio/wav or audio/mpeg)".into(),
            retryable: false,
        }),
    }
}

fn media_io(operation: &str, error: std::io::Error) -> Error {
    Error::Model {
        code: "media_io_failed",
        message: format!("{operation} failed: {}", error.kind()),
        retryable: false,
    }
}
