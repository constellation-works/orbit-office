//! `office.selection-put`: Record a selection from the app; `clear: true`
//! empties the tray.

use office_core::domain::selection::{self, PutRequest, View, to_json};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "selection-put",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &[
        "clear",
        "object",
        "anchor",
        "quote",
        "quote_sha256",
        "view",
        "sensitivity",
    ],
    run,
);

pub(crate) fn run(call: &Call<'_>) -> Result<Value> {
    let clear = bool_field(call.input, "clear")?;
    let object = string_field(call.input, "object")?;
    let anchor = string_field(call.input, "anchor")?;
    let quote = string_field(call.input, "quote")?;
    let quote_sha256 = string_field(call.input, "quote_sha256")?;
    let view = string_field(call.input, "view")?;
    let sensitivity = string_field(call.input, "sensitivity")?;
    let selection = if object.is_none()
        && anchor.is_none()
        && quote.is_none()
        && quote_sha256.is_none()
        && view.is_none()
        && sensitivity.is_none()
    {
        if !clear {
            return Err(Error::InvalidArgument(
                "selection-put requires object and view, or clear: true".into(),
            ));
        }
        None
    } else {
        let object = object.ok_or_else(|| {
            Error::InvalidArgument("selection-put object must be a string".into())
        })?;
        let view = view
            .ok_or_else(|| Error::InvalidArgument("selection-put view must be a string".into()))?;
        let view = View::parse(&view).ok_or_else(|| {
            Error::InvalidArgument(
                "selection-put view must be source, page, artifact or task".into(),
            )
        })?;
        let sensitivity = match sensitivity.as_deref() {
            None => selection::default_sensitivity(),
            Some(token) => selection::parse_sensitivity(token).ok_or_else(|| {
                Error::InvalidArgument("selection-put sensitivity must be normal or private".into())
            })?,
        };
        Some(
            selection::normalize(&object, anchor, quote, quote_sha256, view, sensitivity)
                .map_err(|error| Error::InvalidArgument(error.to_string()))?,
        )
    };
    let result = call
        .ports
        .selections
        .put(&PutRequest { clear, selection }, call.ports.clock.now())?;
    Ok(json!({
        "cleared": result.cleared,
        "evicted": result.evicted,
        "selection": result.selection.as_ref().map(to_json),
    }))
}

fn bool_field(input: &Map<String, Value>, key: &str) -> Result<bool> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(Error::InvalidArgument(format!(
            "selection-put {key} must be a boolean"
        ))),
    }
}

fn string_field(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(Error::InvalidArgument(format!(
            "selection-put {key} must be a string"
        ))),
    }
}
