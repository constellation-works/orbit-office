//! `office.status`: the plugin version, which declared secrets are set
//! (never a secret value; STD-05), Library and index health, the
//! configured private endpoint (`open_weights`, or null), the shared library
//! (`shared`, or null), the followed workspaces (`brief`, or null) and the
//! effective scribe cadence and tags (`scribe`; the defaults until the
//! Library sets them).
//! The token is only `set` or `unset` under `secrets`, and
//! `open_weights_token_binding` says whether it was saved for the configured
//! endpoint's origin (never the token or that origin).
//! Reading creates no Library state.

use office_core::domain::endpoint::{Capability, TokenBinding};
use office_core::domain::settings::{BriefSettings, OpenWeights, Settings};
use office_core::domain::sharing::SharedSettings;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Result, SECRET_NAMES, VERSION};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "status",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let delivered = call.context.get("secrets").and_then(Value::as_object);
    // Presence mirrors model routing (`crates/office/src/app.rs`
    // `client_with_settings`): only a non-empty string counts as set, so an
    // empty stored secret reads as unset everywhere status is shown.
    let is_set = |name: &str| {
        delivered
            .and_then(|secrets| secrets.get(name))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    };
    // STD-01@2 §R35: `baseten_token` was renamed to `open_weights_token`;
    // the old name stays as a deprecated alias for one release, mirroring
    // the new field, and a context still carrying only the old name
    // with a non-empty value counts as set. The alias is removed after that
    // release.
    let open_weights_present = is_set("open_weights_token") || is_set("baseten_token");
    let mut secrets: Map<String, Value> = SECRET_NAMES
        .iter()
        .map(|name| {
            let state = if *name == "open_weights_token" {
                if open_weights_present { "set" } else { "unset" }
            } else if is_set(name) {
                "set"
            } else {
                "unset"
            };
            ((*name).to_string(), json!(state))
        })
        .collect();
    secrets.insert(
        "baseten_token".to_string(),
        json!(if open_weights_present { "set" } else { "unset" }),
    );
    // An unavailable Library adapter (no roots in context) reads as
    // uninitialized health; status itself never refuses for that. Any other
    // failure, such as an invalid config or a pending write, is returned as
    // its typed error rather than reported as an uninitialized Library.
    let health = match call.ports.library.health() {
        Ok(health) => health,
        Err(office_core::Error::LibraryUnavailable) => {
            office_core::ports::LibraryHealth::uninitialized()
        }
        Err(error) => return Err(error),
    };
    // An uninitialized Library has no settings file. A configured one is
    // read afresh; a refusal (invalid config, pending write) stays an error
    // rather than looking like "no endpoint".
    let settings = if health.initialized {
        Some(call.ports.library.settings()?)
    } else {
        None
    };
    let endpoint = settings
        .as_ref()
        .and_then(|settings| settings.open_weights.as_ref())
        .map(|block| block.endpoint.as_str());
    // Routing reads only `open_weights_token`, so a context still carrying
    // only the retired alias has no token any call would send.
    let stored = call
        .context
        .get("secrets")
        .and_then(|secrets| secrets.get("open_weights_token"))
        .and_then(Value::as_str);
    let binding = if open_weights_present && stored.is_none() {
        TokenBinding::Unbound
    } else {
        TokenBinding::of(stored, endpoint)
    };
    let (open_weights, shared, brief, meetings) = match &settings {
        Some(settings) => (
            project_open_weights(settings.open_weights.as_ref()),
            project_shared(settings.shared.as_ref()),
            project_brief(&settings.brief),
            json!({"agent_workspace": &settings.meetings.agent_workspace}),
        ),
        None => (Value::Null, Value::Null, Value::Null, Value::Null),
    };
    // Scribe policy is never absent: an unset or uninitialized Library
    // reads as the shipped defaults.
    let scribe = settings.as_ref().map_or_else(
        || Settings::default().scribe.effective(),
        |s| s.scribe.effective(),
    );
    Ok(json!({
        "version": VERSION,
        "secrets": secrets,
        "open_weights_token_binding": binding.as_str(),
        "library": {
            "initialized": health.initialized,
            "documents": health.documents_total,
            "documents_by_kind": {
                "source": health.source,
                "domain": health.domain,
                "runbook": health.runbook,
                "artifact": health.artifact,
            },
            "pages_off_template": health.pages_off_template,
            "template_invalid": health.template_invalid,
        },
        "open_weights": open_weights,
        "shared": shared,
        "brief": brief,
        "meetings": meetings,
        "scribe": scribe,
    }))
}

/// The validated `shared` block. Its remote never carries credentials: the
/// settings loader refuses one that does.
fn project_shared(block: Option<&SharedSettings>) -> Value {
    block.map_or(Value::Null, |block| {
        json!({
            "remote": &block.remote,
            "branch": &block.branch,
            "member": &block.member,
        })
    })
}

/// Public endpoint fields only. Capabilities and `verified_at` are null
/// until a check has recorded them. The token is never copied here.
fn project_open_weights(block: Option<&OpenWeights>) -> Value {
    let Some(block) = block else {
        return Value::Null;
    };
    let capabilities = block.capabilities.as_ref().map(|caps| {
        caps.iter()
            .copied()
            .map(Capability::as_str)
            .collect::<Vec<&str>>()
    });
    json!({
        "endpoint": &block.endpoint,
        "model": &block.model,
        "kind": block.kind.as_str(),
        "label": &block.label,
        "capabilities": capabilities,
        "verified_at": &block.verified_at,
    })
}

/// The followed extra workspace selectors (`brief.task_workspaces`), exactly
/// as configured and in order, with no resolution or reachability check.
/// An unset list reads as an empty `task_workspaces`.
fn project_brief(block: &BriefSettings) -> Value {
    json!({
        "task_workspaces": &block.task_workspaces,
    })
}
