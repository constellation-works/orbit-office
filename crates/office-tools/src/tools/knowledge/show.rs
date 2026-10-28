//! `office.knowledge-show`: Read a page and its back-links.
//!
//! The page is returned as its model view (contact values withheld), since
//! this tool is visible to agents over MCP (knowledge-templates.md,
//! "Contact details"). Back-links are the indexed sources, pages and
//! artifacts whose frontmatter `cites` or `related` name the page.

use std::path::Path;

use office_core::domain::knowledge::{Page, parse_citation};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "knowledge-show",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["id", "history"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let history = crate::history::requested(call.input)?;
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            parse_citation(id).is_some_and(|c| {
                c.anchor.is_none()
                    && matches!(
                        c.kind,
                        office_core::domain::knowledge::DocumentKind::Domain
                            | office_core::domain::knowledge::DocumentKind::Runbook
                    )
            })
        })
        .ok_or_else(|| {
            Error::InvalidArgument("knowledge-show id must be dom-<slug> or rb-<slug>".into())
        })?;
    let library = &call.ports.library;
    let record = library
        .get(id)?
        .ok_or_else(|| Error::PageNotFound { id: id.into() })?;
    let path = record.path.as_str();
    let markdown = library.read_text(Path::new(path))?;
    let mut view = Page::from_markdown(&markdown)?.model_view();
    view.path = path.to_string();
    let backlinks = library.backlinks(id)?;
    let mut output = json!({
        "page": view,
        "backlinks": backlinks,
    });
    output["page"]["layer"] = json!(record.layer);
    output["page"]["shared_by"] = json!(record.shared_by);
    if history {
        crate::history::append(library.as_ref(), Path::new(path), &mut output)?;
    }
    Ok(output)
}
