//! `office.private-route-check`: check the configured private endpoint
//! with a tiny text, image and audio request and record which it accepts
//! (specs/models-and-privacy.md, "Checking an endpoint").
//!
//! The Models port sends the three requests to the endpoint in the
//! Library's `open_weights` block; the Library port records the accepted
//! capabilities and `verified_at` in `.office/config.yaml` and commits.
//! The output reports the model names and how long each accepted request
//! took; `verified_at` lives in the settings it was recorded in.
//! Nothing routes to an endpoint whose recorded capabilities lack `text`.

use office_core::domain::endpoint::{Capability, ModelRoute};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "private-route-check",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let route = call
        .ports
        .library
        .settings()?
        .open_weights
        .ok_or_else(|| Error::Model {
            code: "private_endpoint_unconfigured",
            message: "set the open_weights block (endpoint, model, kind, label) with office.config-set before checking it".into(),
            retryable: false,
        })?;
    let check = call.ports.models.check_private_route()?;
    let capabilities: Vec<Capability> = check
        .probes
        .iter()
        .filter(|probe| probe.accepted)
        .map(|probe| probe.capability)
        .collect();
    let verified_at = format_rfc3339_utc(call.ports.clock.now());
    // Recorded against the endpoint and model the requests went to; refused
    // if the settings changed meanwhile.
    call.ports.library.record_route_check(
        &check.endpoint,
        &check.model,
        &capabilities,
        &verified_at,
    )?;
    let checks: Vec<Value> = check
        .probes
        .iter()
        .map(|probe| {
            json!({
                "capability": probe.capability.as_str(),
                "accepted": probe.accepted,
                "elapsed_ms": probe.elapsed_ms,
                "error": probe.error,
                "note": (!probe.accepted).then(|| refusal_note(probe.capability)),
            })
        })
        .collect();
    Ok(json!({
        "path": ".office/config.yaml",
        "label": route.label,
        "kind": route.kind.as_str(),
        "route": ModelRoute::Private(route.kind).to_string(),
        "model": check.model,
        "reported_model": check.reported_model,
        "capabilities": capabilities.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
        "routable": capabilities.contains(&Capability::Text),
        "checks": checks,
    }))
}

/// What a failed request means for the user, in plain words.
fn refusal_note(capability: Capability) -> &'static str {
    match capability {
        Capability::Text => {
            "this server didn't answer a text request, so nothing private is sent to it"
        }
        Capability::Image => {
            "this server doesn't take images, so private frames and receipts can't be read"
        }
        Capability::Audio => {
            "this server doesn't take audio, so private videos can't be transcribed"
        }
    }
}
