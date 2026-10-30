//! Calendar model view: private contact values never cross the output boundary.
use crate::registry::{Call, Tool};
use office_core::{
    Error, Result,
    domain::{
        calendar::{Window, plain},
        knowledge::{Page, RefTarget},
        metrics::metadata,
    },
    tool::{ExecutionKind, McpScope, ToolSpec},
};
use serde_json::{Value, json};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "calendar-list",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["from", "until", "include_cancelled"],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let text = |key: &str| {
        call.input
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| Error::InvalidArgument(format!("calendar-list requires RFC 3339 {key}")))
    };
    let window = Window::new(text("from")?, text("until")?, Some(42))?;
    let include = match call.input.get("include_cancelled") {
        None => false,
        Some(Value::Bool(v)) => *v,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "include_cancelled must be a boolean".into(),
            ));
        }
    };
    let (snapshot, last_error) = call.ports.calendar.load()?;
    let selected: Vec<_> = snapshot
        .events
        .iter()
        .filter(|e| window.contains(e) && (include || e.status != "cancelled"))
        .collect();
    let mut events = Vec::new();
    if !selected.is_empty() {
        let pages = call
            .ports
            .library
            .knowledge_pages()?
            .iter()
            .map(|f| Page::from_markdown(markdown(f)?))
            .collect::<Result<Vec<_>>>()?;
        let artifacts = call
            .ports
            .library
            .artifact_documents()?
            .iter()
            .map(|f| Ok((f.path.clone(), metadata(markdown(f)?)?)))
            .collect::<Result<Vec<_>>>()?;
        let sources = call
            .ports
            .library
            .source_documents()?
            .iter()
            .map(|f| Ok((f.path.clone(), metadata(markdown(f)?)?)))
            .collect::<Result<Vec<_>>>()?;
        let addresses: Vec<_> = snapshot
            .events
            .iter()
            .flat_map(|e| e.attendees.iter().filter_map(|a| a.email.as_deref()))
            .collect();
        for event in selected {
            let emails: Vec<_> = event
                .attendees
                .iter()
                .filter_map(|a| a.email.as_deref())
                .collect();
            let mut view = serde_json::to_value(event)
                .map_err(|_| Error::InvalidArgument("cannot encode calendar view".into()))?;
            for a in view["attendees"].as_array_mut().into_iter().flatten() {
                if let Some(map) = a.as_object_mut() {
                    map.remove("email");
                }
            }
            let mut people: Vec<_> = pages
                .iter()
                .filter(|p| {
                    p.template.as_deref() == Some("person")
                        && (event.attendees.iter().any(|a| {
                            a.email.is_none()
                                && (p.title.eq_ignore_ascii_case(&a.name)
                                    || p.aliases
                                        .iter()
                                        .any(|alias| alias.eq_ignore_ascii_case(&a.name)))
                        }) || p.refs.iter().any(|r| {
                            r.target == RefTarget::Contact
                                && emails.iter().any(|email| {
                                    r.value
                                        .trim_start_matches("mailto:")
                                        .eq_ignore_ascii_case(email)
                                })
                        }))
                })
                .map(|p| json!({"id":p.id.as_str(),"title":plain(&p.title)}))
                .collect();
            people.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            view["local"] = json!(event.local);
            view["people"] = json!(people);
            view["prep"] = artifacts
                .iter()
                .find(|(_, m)| {
                    m.get("series").and_then(Value::as_str) == Some("meeting-prep")
                        && event_key(m) == Some(event.key.as_str())
                })
                .and_then(|(_, m)| m.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            view["capture"] = match sources
                .iter()
                .find(|(_, m)| event_key(m) == Some(event.key.as_str()))
            {
                None => Value::Null,
                Some((_, m)) => {
                    let id = m
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::InvalidArgument("meeting source has no id".into()))?;
                    let summarized = call
                        .ports
                        .library
                        .source_sidecar(id, "summary.md")?
                        .is_some();
                    json!({"source":id,"summarized":summarized})
                }
            };
            // Also scrub an address repeated by an invite in its agenda/name/title.
            scrub(&mut view, &addresses);
            events.push(view);
        }
    }
    Ok(
        json!({"events":events,"synced_at":snapshot.synced_at,"last_error":last_error,"sources":call.ports.calendar.sources()?}),
    )
}
fn event_key(map: &serde_json::Map<String, Value>) -> Option<&str> {
    map.get("event")
        .and_then(|v| v.get("key"))
        .and_then(Value::as_str)
        .or_else(|| map.get("event_key").and_then(Value::as_str))
}
fn markdown(file: &office_core::ports::LibraryText) -> Result<&str> {
    file.markdown.as_deref().ok_or(Error::Library {
        code: "library_document_unreadable",
        message: "calendar needs readable Library metadata".into(),
    })
}
fn scrub(value: &mut Value, emails: &[&str]) {
    match value {
        Value::String(text) => {
            for email in emails {
                if email.is_empty() {
                    continue;
                }
                while let Some(start) = text.to_ascii_lowercase().find(&email.to_ascii_lowercase())
                {
                    text.replace_range(start..start + email.len(), "[contact withheld]");
                }
            }
        }
        Value::Array(xs) => {
            for v in xs {
                scrub(v, emails);
            }
        }
        Value::Object(m) => {
            for v in m.values_mut() {
                scrub(v, emails);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
