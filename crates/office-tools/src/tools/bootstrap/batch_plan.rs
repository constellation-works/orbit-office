//! `office.batch-plan`: local cost/route preview and digest-bound confirmations.
use office_core::domain::bootstrap::{self, Mutation};
use office_core::domain::source::{format_rfc3339_utc, parse_source_meta, video};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::Value;

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "batch-plan",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &[
        "phase",
        "confirm",
        "digest",
        "sensitivity",
        "additional_careful",
    ],
    run,
);

struct Input {
    phase: String,
    confirm: bool,
    digest: Option<String>,
    sensitivity: Option<String>,
    additional_careful: bool,
}
fn run(call: &Call<'_>) -> Result<Value> {
    let boolean = |key: &str| match call.input.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(Error::InvalidArgument(format!("{key} must be a boolean"))),
    };
    let string = |key: &str| match call.input.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(Error::InvalidArgument(format!(
            "{key} must be a nonempty string"
        ))),
    };
    let input = Input {
        phase: string("phase")?
            .ok_or_else(|| Error::InvalidArgument("batch-plan requires phase".into()))?,
        confirm: boolean("confirm")?,
        digest: string("digest")?,
        sensitivity: string("sensitivity")?,
        additional_careful: boolean("additional_careful")?,
    };
    bootstrap::phase(&input.phase)?;
    if input.confirm != input.digest.is_some()
        || input.digest.as_ref().is_some_and(String::is_empty)
    {
        return Err(Error::InvalidArgument(
            "confirmation requires confirm: true and the preview digest".into(),
        ));
    }
    if input
        .sensitivity
        .as_deref()
        .is_some_and(|value| value != "private")
        || (input.additional_careful && input.sensitivity.is_some())
    {
        return Err(Error::InvalidArgument(
            "sensitivity may only mark a batch private at its initial preview".into(),
        ));
    }
    let snapshot = call.ports.library.bootstrap_snapshot()?;
    let settings = call.ports.library.settings()?;
    let revision = snapshot.revision(&settings)?;
    let at = format_rfc3339_utc(call.ports.clock.now());
    let mutation = if input.additional_careful {
        Mutation::Extra {
            phase: input.phase,
            confirm: input.digest,
            at,
        }
    } else {
        let private = input.sensitivity.is_some()
            || (input.confirm
                && snapshot
                    .batches
                    .get(&input.phase)
                    .is_some_and(|batch| batch.plan.private));
        let mut videos = Vec::new();
        for source in &snapshot.sources {
            if source.in_phase(&input.phase)? && source.digest.is_none() && !source.needs_ocr()? {
                let meta = parse_source_meta(&source.markdown);
                if meta.kind == "video" {
                    let video_source = call.ports.library.read_video(&meta.id)?;
                    // A different source revision must not be hidden by local video inspection.
                    if video_source.markdown != source.markdown {
                        return Err(video::refusal(
                            "plan_changed",
                            "video changed during batch preview",
                        ));
                    }
                    let analysis = call
                        .ports
                        .models
                        .analyze_video(&video_source.bytes, &settings.video.scene_threshold)?;
                    videos.push(video::plan(
                        &video_source,
                        &analysis,
                        &settings,
                        if private {
                            "private"
                        } else {
                            &meta.sensitivity
                        },
                    )?);
                }
            }
        }
        Mutation::Plan {
            plan: Box::new(bootstrap::plan(
                &snapshot,
                &settings,
                &input.phase,
                private,
                videos,
            )?),
            confirm: input.digest,
            at,
        }
    };
    let batch = call.ports.library.mutate_batch(&revision, mutation)?;
    serde_json::to_value(batch)
        .map_err(|_| video::refusal("invalid_bootstrap_data", "cannot encode batch response"))
}
