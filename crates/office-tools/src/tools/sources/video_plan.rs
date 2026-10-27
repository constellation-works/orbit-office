//! `office.video-plan`: local preview and digest-bound confirmation, without inference.
use crate::registry::{Call, Tool};
use office_core::domain::source::{format_rfc3339_utc, parse_source_meta, sha256_hex, video};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "video-plan",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["id", "confirm", "digest", "sensitivity"],
    run,
);
/// Maximum plans one list call returns, matching the routine fan-out bound.
pub(crate) const MAX_PENDING_PLANS: usize = 256;

fn list(call: &Call<'_>) -> Result<Value> {
    if !call.input.is_empty() {
        return Err(Error::InvalidArgument(
            "video-plan list mode takes no input; omit id, confirm, digest and sensitivity".into(),
        ));
    }
    let snapshot = call.ports.library.bootstrap_snapshot()?;
    let mut plans = Vec::new();
    for source in &snapshot.sources {
        let meta = office_core::domain::source::parse_source_meta(&source.markdown);
        if meta.kind != "video" {
            continue;
        }
        if let Some(state) = video::state(&source.markdown)?
            && video::is_pending(&state)
        {
            plans.push(json!({"source_id":meta.id,"plan_digest":state.plan.digest}));
        }
    }
    plans.sort_by(|a, b| a["source_id"].as_str().cmp(&b["source_id"].as_str()));
    plans.truncate(MAX_PENDING_PLANS);
    Ok(json!({"plans":plans}))
}
fn run(call: &Call<'_>) -> Result<Value> {
    if !call.input.contains_key("id") {
        return list(call);
    }
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::InvalidArgument("video-plan requires a source id".into()))?;
    let confirm = match call.input.get("confirm") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(Error::InvalidArgument("confirm must be a boolean".into())),
    };
    let digest = match call.input.get("digest") {
        None => None,
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "digest must be a nonempty string".into(),
            ));
        }
    };
    if confirm != digest.is_some() {
        return Err(Error::InvalidArgument(
            "confirmation requires confirm: true and the preview digest".into(),
        ));
    }
    let source = call.ports.library.read_video(id)?;
    let meta = parse_source_meta(&source.markdown);
    let sensitivity = match call.input.get("sensitivity") {
        None => meta.sensitivity.as_str(),
        Some(Value::String(value)) if value == "normal" || value == "private" => value.as_str(),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "sensitivity must be normal or private".into(),
            ));
        }
    };
    let settings = call.ports.library.settings()?;
    let analysis = call
        .ports
        .models
        .analyze_video(&source.bytes, &settings.video.scene_threshold)?;
    let plan = video::plan(&source, &analysis, &settings, sensitivity)?;
    let state = call.ports.library.mutate_video(
        id,
        &sha256_hex(source.markdown.as_bytes()),
        video::VideoMutation::Plan {
            plan: Box::new(plan),
            confirm_digest: digest,
            at: format_rfc3339_utc(call.ports.clock.now()),
        },
    )?;
    Ok(
        json!({"plan":state.plan,"confirmed_at":state.confirmed_at,"calls":state.calls,"frames_read":state.frames_read}),
    )
}
