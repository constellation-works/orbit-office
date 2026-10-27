use super::{Attendee, CalendarError, Event, Organizer, parse_time, plain, stamp, zone};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone};
use serde_json::{Map, Value, json};
use url::Url;

fn datetime(value: &Value) -> Result<DateTime<FixedOffset>, CalendarError> {
    let text = value
        .get("dateTime")
        .and_then(Value::as_str)
        .ok_or(CalendarError::Invalid)?;
    if let Ok(t) = parse_time(text) {
        return Ok(t);
    }
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .map_err(|_| CalendarError::Invalid)?;
    zone(
        value
            .get("timeZone")
            .and_then(Value::as_str)
            .ok_or(CalendarError::Zone)?,
    )?
    .from_local_datetime(&naive)
    .earliest()
    .map(|t| t.fixed_offset())
    .ok_or(CalendarError::Zone)
}
fn event(v: &Value) -> Result<Event, CalendarError> {
    let start = datetime(&v["start"])?;
    let end = datetime(&v["end"])?;
    if end < start {
        return Err(CalendarError::Invalid);
    }
    let uid = v["iCalUId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(CalendarError::Invalid)?;
    let original = match v.get("originalStart") {
        None | Some(Value::Null) => start,
        Some(Value::String(s)) => parse_time(s)?,
        Some(_) => return Err(CalendarError::Invalid),
    };
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let attendees = v
        .get("attendees")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|a| Attendee {
                    name: plain(&text(&a["emailAddress"]["name"])),
                    email: a["emailAddress"]["address"]
                        .as_str()
                        .map(|s| s.to_ascii_lowercase()),
                    response: text(&a["status"]["response"]),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Event {
        key: format!("{uid}:{}", stamp(original)),
        series: uid.into(),
        title: plain(&text(&v["subject"])),
        start: start.to_rfc3339(),
        end: end.to_rfc3339(),
        all_day: v["isAllDay"].as_bool().unwrap_or(false),
        status: if v["isCancelled"].as_bool() == Some(true) {
            "cancelled"
        } else if v["showAs"] == "tentative" {
            "tentative"
        } else {
            "confirmed"
        }
        .into(),
        location: plain(&text(&v["location"]["displayName"])),
        online: v["isOnlineMeeting"].as_bool() == Some(true)
            || v["onlineMeeting"]["joinUrl"].as_str().is_some(),
        organizer: Organizer {
            name: plain(&text(&v["organizer"]["emailAddress"]["name"])),
        },
        attendees,
        agenda: plain(&text(&v["body"]["content"])),
        sensitivity: if ["private", "confidential"]
            .contains(&v["sensitivity"].as_str().unwrap_or(""))
        {
            "private"
        } else {
            "normal"
        }
        .into(),
        recurrence_unsupported: false,
    })
}
/// Normalize one Graph page; unresolved timezone events are counted and skipped.
pub fn graph_events(page: &Value) -> Result<(Vec<Event>, usize), CalendarError> {
    let mut events = Vec::new();
    let mut skipped = 0;
    for value in page
        .get("value")
        .and_then(Value::as_array)
        .ok_or(CalendarError::Invalid)?
    {
        match event(value) {
            Ok(e) => events.push(e),
            Err(CalendarError::Zone) => skipped += 1,
            Err(e) => return Err(e),
        }
    }
    Ok((events, skipped))
}
/// Turn an absolute next link into a gateway path/query, refusing any base escape.
/// No URL or credential is forwarded to the gateway as a raw request target.
pub fn next_request(
    base: &str,
    next: &str,
    connection: &str,
) -> Result<Map<String, Value>, CalendarError> {
    let refused = || CalendarError::NextLink;
    let base = Url::parse(base).map_err(|_| refused())?;
    let next = Url::parse(next).map_err(|_| refused())?;
    let prefix = format!("{}/", base.path().trim_end_matches('/'));
    if base.scheme() != "https"
        || base.origin() != next.origin()
        || !base.username().is_empty()
        || base.password().is_some()
        || !next.username().is_empty()
        || next.password().is_some()
        || next.fragment().is_some()
        || !next.path().starts_with(&prefix)
    {
        return Err(refused());
    }
    let path = &next.path()[prefix.len()..];
    // Gateway accepts literal relative segments; encoded separators and traversal
    // are refused rather than decoded into a different path by a downstream server.
    if path.contains('%') || path.contains('\\') || path.is_empty() {
        return Err(refused());
    }
    let mut query = Map::new();
    for (key, value) in next.query_pairs() {
        if query.insert(key.into_owned(), json!(value)).is_some() {
            return Err(refused());
        }
    }
    let mut out = Map::new();
    out.insert("connection".into(), json!(connection));
    out.insert("path".into(), json!(path));
    out.insert("query".into(), Value::Object(query));
    Ok(out)
}
