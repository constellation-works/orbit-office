//! `office.source-show`: Return a source, optionally at an anchor.

use office_core::domain::source::{anchor_span, parse_anchor};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "source-show",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["id", "source_id", "anchor"],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let id = match (call.input.get("id"), call.input.get("source_id")) {
        (Some(Value::String(id)), _) if !id.trim().is_empty() => id.clone(),
        (_, Some(Value::String(id))) if !id.trim().is_empty() => id.clone(),
        _ => {
            return Err(Error::InvalidArgument(
                "source-show requires a source id".to_string(),
            ));
        }
    };
    let anchor = match call.input.get("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "source-show anchor must be a string".to_string(),
            ));
        }
    };
    let view = call.ports.library.read_source(&id)?;
    let (span, resolved_anchor) = match &anchor {
        Some(raw) => {
            let parsed = parse_anchor(raw)?;
            (
                Some(anchor_span(&view.body, parsed, raw)?),
                Some(raw.clone()),
            )
        }
        None => (None, None),
    };
    let body = span.clone().unwrap_or_else(|| view.body.clone());
    Ok(json!({
        "id": view.id,
        "kind": view.kind,
        "title": view.title,
        "path": view.path,
        "body": body,
        "text": body,
        "span": span,
        "anchor": resolved_anchor,
        "sensitivity": view.sensitivity,
        "sha256": view.sha256,
        "bytes": view.bytes,
        "media_type": view.media_type,
        "version": view.version,
    }))
}
