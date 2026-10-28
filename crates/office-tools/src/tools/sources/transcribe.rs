//! Bounded media transcription, composed through Library and Models ports.
use crate::registry::{Call, Tool};
use office_core::domain::source::{imported_vtt, parse_source_meta, sha256_hex, video};
use office_core::ports::{TranscribeRequest, Transcript};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "transcribe",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["id", "duration_seconds", "language"],
    run,
);
pub(super) fn run(call: &Call<'_>) -> Result<Value> {
    let id = required_string(call, "id")?;
    let language = call
        .input
        .get("language")
        .map(|value| {
            value
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Error::InvalidArgument("language must be a nonempty string".into()))
        })
        .transpose()?;
    let requested_duration = call
        .input
        .get("duration_seconds")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0 && *n <= 604_800)
                .ok_or_else(|| {
                    Error::InvalidArgument(
                        "duration_seconds must be an integer from 1 to 604800".into(),
                    )
                })
        })
        .transpose()?;
    let view = call.ports.library.read_source(id)?;
    let markdown = call.ports.library.read_text(Path::new(&view.path))?;
    if imported_vtt(&markdown)? {
        return Ok(json!({"id":id,"path":view.path,"version":view.version,"noop":true,"chunks":0}));
    }
    let source = call.ports.library.read_media(id)?;
    let meta = parse_source_meta(&source.markdown);
    let state = if meta.kind == "video" {
        Some(approved(call, id)?)
    } else {
        None
    };
    let duration = if let Some(state) = &state {
        state.plan.duration_ms.div_ceil(1000)
    } else {
        requested_duration.ok_or_else(|| {
            Error::InvalidArgument("meeting transcription requires duration_seconds".into())
        })?
    };
    if let Some(state) = &state {
        if !state.plan.transcribe_audio {
            return Ok(
                json!({"id":id,"path":view.path,"version":view.version,"noop":true,"chunks":0}),
            );
        }
        if state.calls.audio != 0 {
            return Err(video::refusal(
                "plan_exceeded",
                "audio budget was already consumed; preview and confirm a new plan",
            ));
        }
    }
    let sensitivity = serde_json::from_value(json!(view.sensitivity))
        .map_err(|_| video::refusal("invalid_source", "invalid source sensitivity"))?;
    let mut blocks = Vec::new();
    let mut latest = view;
    let count = duration.div_ceil(600);
    for index in 0..count {
        let start = index * 600;
        let length = (duration - start).min(600);
        let audio = call
            .ports
            .models
            .media_audio(&source.bytes, start, length)?;
        if let Some(state) = &state {
            reserve(
                call,
                id,
                "audio",
                usize::try_from(index)
                    .map_err(|_| video::refusal("plan_exceeded", "chunk count overflow"))?,
                &state.plan.digest,
            )?;
        }
        let current = if state.is_some() || index > 0 {
            call.ports.library.read_text(Path::new(&latest.path))?
        } else {
            source.markdown.clone()
        };
        let request = TranscribeRequest {
            audio,
            duration_seconds: length,
            mime: "audio/wav".into(),
            sensitivity,
            language: language.clone(),
        };
        let reply = if let Some(state) = &state {
            call.ports
                .models
                .transcribe_planned(&request, &state.plan.settings_digest)?
        } else {
            call.ports.models.transcribe(&request)?
        };
        blocks.extend(turns(&reply.value, start, length)?);
        latest = call.ports.library.write_source_processing(
            id,
            &sha256_hex(current.as_bytes()),
            Some(&format!("{}\n", blocks.join("\n\n"))),
            &reply.derived_by,
            &if meta.kind == "meeting" {
                vec![(
                    "transcript-state.json".into(),
                    json!({"complete": index + 1 == count})
                        .to_string()
                        .into_bytes(),
                )]
            } else {
                vec![]
            },
        )?;
    }
    Ok(json!({"id":id,"path":latest.path,"version":latest.version,"noop":false,"chunks":count}))
}
pub(super) fn required_string<'a>(call: &'a Call<'_>, key: &str) -> Result<&'a str> {
    call.input
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Error::InvalidArgument(format!("{key} must be a nonempty string")))
}
pub(super) fn approved(call: &Call<'_>, id: &str) -> Result<video::VideoState> {
    let source = call.ports.library.read_video(id)?;
    let state = video::state(&source.markdown)?
        .filter(|s| s.confirmed_at.is_some())
        .ok_or_else(|| {
            video::refusal(
                "plan_unconfirmed",
                "preview and confirm the video plan first",
            )
        })?;
    video::verify(&source, &call.ports.library.settings()?, &state)?;
    Ok(state)
}
pub(super) fn reserve(
    call: &Call<'_>,
    id: &str,
    group: &str,
    index: usize,
    expected_digest: &str,
) -> Result<()> {
    let state = approved(call, id)?;
    if state.plan.digest != expected_digest {
        return Err(video::refusal(
            "plan_changed",
            "confirmed plan changed during processing",
        ));
    }
    let source = call.ports.library.read_video(id)?;
    call.ports.library.mutate_video(
        id,
        &sha256_hex(source.markdown.as_bytes()),
        video::VideoMutation::ReserveProcessing {
            digest: expected_digest.into(),
            group: group.into(),
            index,
        },
    )?;
    Ok(())
}
fn turns(transcript: &Transcript, offset: u64, length: u64) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if transcript.segments.is_empty() {
        // Missing timestamps get deterministic positional anchors, not claimed
        // speech alignment: line N starts N seconds into the decoded window.
        for (index, line) in transcript
            .text
            .lines()
            .filter(|s| !s.trim().is_empty())
            .enumerate()
        {
            let position = u64::try_from(index).unwrap_or(u64::MAX).min(length - 1);
            let (speaker, text) = line.split_once(": ").unwrap_or(("Speaker", line));
            out.push(turn(offset + position, speaker, text));
        }
    } else {
        for segment in &transcript.segments {
            if segment.start_ms >= length * 1000
                || segment.end_ms < segment.start_ms
                || segment.end_ms > length * 1000
            {
                return Err(video::refusal(
                    "model_invalid_response",
                    "transcript turn lies outside its audio window",
                ));
            }
            out.push(turn(
                offset + segment.start_ms / 1000,
                segment.speaker.as_deref().unwrap_or("Speaker"),
                &segment.text,
            ));
        }
    }
    Ok(out)
}
fn turn(seconds: u64, speaker: &str, text: &str) -> String {
    let stamp = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    );
    format!(
        "[{stamp}] {}: {} [turn](#t={stamp})",
        speaker.split_whitespace().collect::<Vec<_>>().join(" "),
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}
