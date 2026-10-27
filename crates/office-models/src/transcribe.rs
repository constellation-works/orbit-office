//! Stage only long recordings, then route every chunk through the same client.

use std::io::Write;

use office_core::ports::{TranscribeRequest, Transcript};
use office_core::{Error, Result};

use crate::{RoutedClient, http, media, meta, routing};

pub(crate) fn transcribe(client: &RoutedClient, request: &TranscribeRequest) -> Result<Transcript> {
    let call = |audio: &[u8], offset_ms: u64| {
        let parsed = client.transcribe_chunk(
            request.sensitivity,
            &request.mime,
            audio,
            request.language.as_deref(),
        )?;
        shift_turns(parsed, offset_ms)
    };
    let splitter = &client.config.audio_splitter;
    if !media::needs_split(request.duration_seconds, splitter.threshold_seconds) {
        return call(&request.audio, 0);
    }

    // Refuse an unusable route before staging private media or starting ffmpeg.
    let resolved = routing::route(request.sensitivity, &client.config)?;
    http::check_url(&meta::chat_url(&resolved.base_url), resolved.open_weights)?;
    let extension = audio_extension(&request.mime)?;
    let directory = tempfile::tempdir().map_err(|error| media_io("create audio staging", error))?;
    let mut input = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
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
    for (path, piece) in paths.iter().zip(plan) {
        let audio = std::fs::read(path).map_err(|error| media_io("read split audio", error))?;
        let offset_ms = piece
            .start_seconds
            .checked_mul(1_000)
            .ok_or_else(timestamp_overflow)?;
        let chunk = call(&audio, offset_ms)?;
        texts.push(chunk.text);
        segments.extend(chunk.segments);
    }
    Ok(Transcript {
        text: texts.join("\n"),
        segments,
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
    Ok(Transcript {
        text: parsed.text,
        segments: parsed.segments,
    })
}

// ffmpeg chooses the output container from this suffix. Never use arbitrary
// caller-supplied MIME text as a path or quietly relabel a different container.
fn audio_extension(mime: &str) -> Result<&'static str> {
    let mime = mime.split(';').next().unwrap_or_default().trim();
    match mime {
        "audio/wav" | "audio/wave" | "audio/x-wav" => Ok("wav"),
        "audio/mpeg" | "audio/mp3" => Ok("mp3"),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Ok("m4a"),
        "audio/aac" => Ok("aac"),
        "audio/flac" | "audio/x-flac" => Ok("flac"),
        "audio/ogg" => Ok("ogg"),
        "audio/webm" => Ok("webm"),
        _ => Err(Error::Model {
            code: "media_unsupported_format",
            message: "long audio needs a WAV, MP3, M4A, AAC, FLAC, Ogg or WebM MIME type".into(),
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
