//! Bounded gateway reads or one contained offline import, then atomic publication.
use crate::registry::{Call, Tool};
use office_core::{
    Error, Result,
    domain::calendar::{self, CalendarError, Snapshot, Window},
    domain::source::format_rfc3339_utc,
    tool::{ExecutionKind, McpScope, ToolSpec},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "calendar-sync",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["ics_file"],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let file = match call.input.get("ics_file") {
        None => None,
        Some(Value::String(s)) if !s.is_empty() => Some(s.as_str()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "ics_file must be a workspace-relative .ics path".into(),
            ));
        }
    };
    let now = format_rfc3339_utc(call.ports.clock.now());
    let result: Result<Value> = (|| {
        let settings = call.ports.library.settings()?.calendar;
        let window = Window::around(&now, &settings)?;
        let from = window.from;
        let until = window.until;
        let (events, skipped) = if let Some(path) = file {
            calendar::parse_ics(&call.ports.calendar.read_ics(path)?, &window)?
        } else {
            fetch(
                call,
                settings
                    .connection
                    .as_deref()
                    .ok_or(CalendarError::Unconfigured)?,
                &window,
            )?
        };
        let snapshot = Snapshot {
            events,
            skipped,
            synced_at: Some(now.clone()),
        };
        call.ports.calendar.replace(&snapshot)?;
        Ok(
            json!({"events":snapshot.events.len(),"skipped":skipped,"window":{"from":calendar::stamp(from),"until":calendar::stamp(until)},"synced_at":now}),
        )
    })();
    if let Err(ref error) = result {
        // Record only a stable code; callback diagnostics may contain secrets.
        call.ports.calendar.record_error(error.code())?;
    }
    result
}
fn get(call: &Call<'_>, input: &Map<String, Value>, limit: u64) -> Result<Value> {
    let reply = call
        .ports
        .orbit
        .gateway_http_get(input)
        .map_err(|_| CalendarError::Fetch)?;
    if reply["truncated"] == true
        || reply["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > limit.min(1_048_576))
    {
        return Err(CalendarError::TooLarge.into());
    }
    if !reply["status"]
        .as_u64()
        .is_some_and(|n| (200..300).contains(&n))
        || reply["truncated"].as_bool() != Some(false)
    {
        return Err(CalendarError::Fetch.into());
    }
    Ok(reply)
}
fn fetch(
    call: &Call<'_>,
    connection: &str,
    window: &Window,
) -> Result<(Vec<calendar::Event>, usize)> {
    let list = call
        .ports
        .orbit
        .gateway_connection_list(&Map::new())
        .map_err(|_| CalendarError::Fetch)?;
    let conn = list["connections"]
        .as_array()
        .and_then(|xs| {
            xs.iter()
                .find(|c| c["name"] == connection && c["disabled"] == false)
        })
        .ok_or(CalendarError::Unconfigured)?;
    let base = conn["base_url"].as_str().ok_or(CalendarError::Invalid)?;
    let limit = conn["max_response_bytes"]
        .as_u64()
        .ok_or(CalendarError::Invalid)?;
    let mut input = Map::new();
    input.insert("connection".into(), json!(connection));
    if conn["auth"]["scheme"] != "oauth2_device" {
        input.insert("path".into(), json!(""));
        let reply = get(call, &input, limit)?;
        let text = reply["body"]["text"]
            .as_str()
            .ok_or(CalendarError::Invalid)?;
        return Ok(calendar::parse_ics(text, window)?);
    }
    input.insert("path".into(), json!("me/calendarView"));
    input.insert("query".into(),json!({"startDateTime":calendar::stamp(window.from),"endDateTime":calendar::stamp(window.until),"$top":1000}));
    let mut events = Vec::new();
    let mut skipped = 0;
    let mut seen = BTreeSet::new();
    for page_index in 0..10 {
        let reply = get(call, &input, limit)?;
        let body = &reply["body"]["value"];
        let (page, count) = calendar::graph_events(body)?;
        skipped += count;
        events.extend(page.into_iter().filter(|e| window.contains(e)));
        if events.len() > 10000 {
            return Err(CalendarError::TooLarge.into());
        }
        match body.get("@odata.nextLink") {
            None | Some(Value::Null) => {
                events.sort_by(|a, b| {
                    calendar::parse_time(&a.start)
                        .ok()
                        .cmp(&calendar::parse_time(&b.start).ok())
                        .then(a.key.cmp(&b.key))
                });
                let mut keys = BTreeSet::new();
                if events.iter().any(|e| !keys.insert(e.key.clone())) {
                    return Err(CalendarError::Invalid.into());
                }
                return Ok((events, skipped));
            }
            Some(Value::String(next)) => {
                input = calendar::next_request(base, next, connection)?;
                if !seen.insert(next.clone()) {
                    return Err(CalendarError::NextLink.into());
                }
                if page_index == 9 {
                    return Err(CalendarError::TooLarge.into());
                }
            }
            Some(_) => return Err(CalendarError::Invalid.into()),
        }
    }
    Err(CalendarError::TooLarge.into())
}
