//! `office.scribe-control`: Office-owned per-session scribe control state
//! (specs/sessions.md, "Live scribe"; specs/recall.md, "Controls").
//!
//! One record per provider session: the switch, a pending Capture-now
//! request, the user's title and the last Scribe window heartbeat. The web
//! Scribe window and the Claude Code plugin's hooks and `/scribe` command
//! read and write it here, so both hosts share one switch; the plugin's
//! flag file is its local cache. Plugin state only: no Library write, no
//! model call, no Orbit callback.

use office_core::domain::scribe_control::{
    Change, parse_time, validate_session_id, validate_title,
};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "scribe-control",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["op", "session_id", "on", "title", "requested_at"],
    run,
);

fn invalid(message: &str) -> Error {
    Error::InvalidArgument(message.into())
}

/// Refuse a field the operation does not take, rather than ignore it.
fn only(input: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    match input.keys().find(|key| {
        !["op", "session_id"].contains(&key.as_str()) && !allowed.contains(&key.as_str())
    }) {
        Some(_) => Err(invalid(
            "scribe-control field is not valid for this op: get, heartbeat and request_now take none; set_switch takes on; set_title takes title; consume_now takes optional requested_at",
        )),
        None => Ok(()),
    }
}

fn change(input: &Map<String, Value>, op: &str) -> Result<Option<Change>> {
    Ok(match op {
        "get" => {
            only(input, &[])?;
            None
        }
        "set_switch" => {
            only(input, &["on"])?;
            let on = input
                .get("on")
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid("set_switch requires on (true or false)"))?;
            Some(Change::SetSwitch(on))
        }
        "request_now" => {
            only(input, &[])?;
            Some(Change::RequestNow)
        }
        "consume_now" => {
            only(input, &["requested_at"])?;
            let expected = match input.get("requested_at") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    Some(parse_time(value.as_str().ok_or_else(|| {
                        invalid("requested_at must be an RFC 3339 timestamp")
                    })?)?)
                }
            };
            Some(Change::ConsumeNow(expected))
        }
        "set_title" => {
            only(input, &["title"])?;
            let title = input
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("set_title requires title"))?;
            validate_title(title)?;
            Some(Change::SetTitle(title.to_owned()))
        }
        "heartbeat" => {
            only(input, &[])?;
            Some(Change::Heartbeat)
        }
        _ => {
            return Err(invalid(
                "op must be get, set_switch, request_now, consume_now, set_title or heartbeat",
            ));
        }
    })
}

fn run(call: &Call<'_>) -> Result<Value> {
    let op = call
        .input
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("scribe-control requires op"))?;
    let session_id = call
        .input
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("scribe-control requires session_id"))?;
    validate_session_id(session_id)?;
    let change = change(call.input, op)?;
    let now = call.ports.clock.now();
    let store = &call.ports.scribe_control;
    let mut view = match &change {
        None => store.get(session_id)?.view(session_id, now),
        Some(change) => {
            let (control, consumed) = store.apply(session_id, change, now)?;
            let mut view = control.view(session_id, now);
            if matches!(change, Change::ConsumeNow(_)) {
                view["consumed"] = Value::Bool(consumed);
            }
            view
        }
    };
    view["op"] = Value::String(op.to_owned());
    Ok(view)
}
