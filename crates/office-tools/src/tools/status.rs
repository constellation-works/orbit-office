//! `office.status`: the plugin version, which declared secrets are set
//! (never a secret value; STD-05), Library and index health, and the
//! configured private endpoint (`open_weights`, or null) and shared library
//! (`shared`, or null). The token is only `set` or `unset` under `secrets`.
//! Reading creates no Library state.

use office_core::domain::endpoint::Capability;
use office_core::domain::settings::OpenWeights;
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
    let present: Vec<&str> = call
        .context
        .get("secrets")
        .and_then(Value::as_object)
        .map(|secrets| secrets.keys().map(String::as_str).collect())
        .unwrap_or_default();
    // STD-01@2 §R35: `baseten_token` was renamed to `open_weights_token`;
    // the old name stays as a deprecated alias for one release, mirroring
    // the new field, and a context still carrying only the old name
    // counts as set. The alias is removed after that release.
    let open_weights_present =
        present.contains(&"open_weights_token") || present.contains(&"baseten_token");
    let mut secrets: Map<String, Value> = SECRET_NAMES
        .iter()
        .map(|name| {
            let state = if *name == "open_weights_token" {
                if open_weights_present { "set" } else { "unset" }
            } else if present.contains(name) {
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
    let (open_weights, shared) = if health.initialized {
        match call.ports.library.settings() {
            Ok(settings) => (
                project_open_weights(settings.open_weights.as_ref()),
                project_shared(settings.shared.as_ref()),
            ),
            Err(error) => return Err(error),
        }
    } else {
        (Value::Null, Value::Null)
    };
    Ok(json!({
        "version": VERSION,
        "secrets": secrets,
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
