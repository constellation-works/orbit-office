//! `office.frames`: reserve each confirmed image call before reading it and persist partial progress.
use crate::registry::{Call, Tool};
use office_core::domain::source::{sha256_hex, video};
use office_core::ports::ImageRequest;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "frames",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["id"],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::InvalidArgument("frames requires a source id".into()))?;
    loop {
        let source = call.ports.library.read_video(id)?;
        let state = video::state(&source.markdown)?
            .filter(|state| state.confirmed_at.is_some())
            .ok_or_else(|| {
                video::refusal(
                    "plan_unconfirmed",
                    "preview and confirm the video plan first",
                )
            })?;
        let settings = call.ports.library.settings()?;
        video::verify(&source, &settings, &state)?;
        if state.calls.frames > state.plan.counts.frames
            || state.frames_read != state.calls.frames
            || state.plan.counts.frames != state.plan.frames.len()
        {
            return Err(video::refusal(
                "plan_exceeded",
                "frame budget consumed or an earlier request has an uncertain result",
            ));
        }
        if state.frames_read == state.plan.frames.len() {
            let parent = source
                .path
                .strip_suffix("source.md")
                .unwrap_or(&source.path);
            return Ok(
                json!({"id":id,"path":format!("{parent}frames.md"),"frames_read":state.frames_read,"calls":state.calls}),
            );
        }
        let index = state.calls.frames;
        let image = call
            .ports
            .models
            .video_frame(&source.bytes, state.plan.frames[index].time_ms)?;
        call.ports.library.mutate_video(
            id,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::Reserve {
                digest: state.plan.digest.clone(),
                index,
            },
        )?;
        let reserved = call.ports.library.read_video(id)?;
        let reserved_state = video::state(&reserved.markdown)?
            .ok_or_else(|| video::refusal("plan_changed", "reserved plan disappeared"))?;
        if reserved_state.plan.digest != state.plan.digest {
            return Err(video::refusal(
                "plan_changed",
                "plan changed after reserving a frame call",
            ));
        }
        let sensitivity = &state.plan.sensitivity;
        let reading = call.ports.models.read_planned_image(&ImageRequest {
            image:image.clone(), mime:"image/png".into(),
            prompt:"Describe what is on screen (slide, code, terminal, dashboard, diagram or face) and transcribe visible text. For a face alone, write one short line.".into(),
            sensitivity:serde_json::from_value(json!(sensitivity)).map_err(|_| video::refusal("invalid_video_source", "invalid source sensitivity"))?,
        }, &state.plan.settings_digest)?;
        call.ports.library.mutate_video(
            id,
            &sha256_hex(reserved.markdown.as_bytes()),
            video::VideoMutation::Frame {
                digest: state.plan.digest,
                index,
                image,
                text: reading.value.text,
                derived_by: reading.derived_by,
            },
        )?;
    }
}
