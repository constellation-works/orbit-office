//! `office.search`: FTS over sources, knowledge and artifacts, with filters
//! by kind, template, tag, series and date (specs/library.md, "Search").
//!
//! Every filter narrows the index query before its limit. A document's date
//! is the one the index records (`DocumentRecord::date`); `since` and
//! `until` are inclusive and exclude undated documents. An empty query is
//! refused: this tool finds material, it does not list the Library.
//!
//! A `private` hit is listed by metadata only: its snippet is emptied and
//! the hit is marked `content_withheld: true`, so private body text never
//! reaches a caller of this MCP-visible tool (the private-content decision
//! in docs/design/office/4_decisions.md).

use office_core::domain::knowledge::{DocumentKind, LibraryLayer, SearchFilter};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "search",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[
        "query", "layer", "kind", "template", "tag", "series", "since", "until", "limit",
    ],
    run,
);

/// Hits returned when `limit` is absent, and the most a call may ask for.
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

fn run(call: &Call<'_>) -> Result<Value> {
    let input = call.input;
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| Error::InvalidArgument("search query must be nonempty text".into()))?;
    let kind = optional_text(input, "kind")?
        .map(|token| {
            DocumentKind::parse(&token).ok_or_else(|| {
                Error::InvalidArgument(
                    "search kind must be source, domain, runbook or artifact".into(),
                )
            })
        })
        .transpose()?;
    let since = optional_date(input, "since")?;
    let until = optional_date(input, "until")?;
    if let (Some(since), Some(until)) = (&since, &until)
        && since > until
    {
        return Err(Error::InvalidArgument(
            "search since must not be after until".into(),
        ));
    }
    let limit = match input.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_LIMIT).contains(limit))
            .ok_or_else(|| {
                Error::InvalidArgument(format!(
                    "search limit must be an integer from 1 to {MAX_LIMIT}"
                ))
            })?,
    };
    let layer = optional_text(input, "layer")?
        .map(|value| match value.as_str() {
            "own" => Ok(LibraryLayer::Own),
            "shared" => Ok(LibraryLayer::Shared),
            _ => Err(Error::InvalidArgument(
                "search layer must be own or shared".into(),
            )),
        })
        .transpose()?;
    let filter = SearchFilter {
        layer,
        kind,
        tag: optional_text(input, "tag")?,
        series: optional_text(input, "series")?,
        template: optional_text(input, "template")?,
        since,
        until,
        limit: Some(limit),
    };
    let hits: Vec<Value> = call
        .ports
        .library
        .search(query, &filter)?
        .into_iter()
        .take(limit)
        .map(|hit| {
            let withheld = hit.sensitivity == "private";
            let mut output = json!({
                "id": hit.id,
                "layer": hit.layer,
                "shared_by": hit.shared_by,
                "kind": hit.kind,
                "title": hit.title,
                "path": hit.path,
                "snippet": if withheld { String::new() } else { hit.snippet },
                "sensitivity": hit.sensitivity,
            });
            if withheld {
                output["content_withheld"] = json!(true);
            }
            output
        })
        .collect();
    Ok(json!({ "count": hits.len(), "hits": hits }))
}

/// A nonempty string field, or `None` when absent or null.
fn optional_text(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(Some(text.trim().to_string())),
        Some(_) => Err(Error::InvalidArgument(format!(
            "search {key} must be nonempty text"
        ))),
    }
}

/// A `YYYY-MM-DD` field, or `None` when absent or null.
fn optional_date(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    let Some(text) = optional_text(input, key)? else {
        return Ok(None);
    };
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    let month = text.get(5..7).and_then(|m| m.parse::<u8>().ok());
    let day = text.get(8..10).and_then(|d| d.parse::<u8>().ok());
    if shaped
        && month.is_some_and(|m| (1..=12).contains(&m))
        && day.is_some_and(|d| (1..=31).contains(&d))
    {
        Ok(Some(text))
    } else {
        Err(Error::InvalidArgument(format!(
            "search {key} must be a YYYY-MM-DD date"
        )))
    }
}
