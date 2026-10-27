//! `office.knowledge-show`: Read a page and its back-links.
//!
//! The page is returned as its model view (contact values withheld), since
//! this tool is visible to agents over MCP (knowledge-templates.md,
//! "Contact details"). Back-links are the indexed sources, pages and
//! artifacts whose frontmatter `cites` or `related` name the page.

use std::path::Path;

use office_core::domain::knowledge::{DocumentRecord, Page, page_kind_for_id};
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
    &["id"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| page_kind_for_id(id).is_some())
        .ok_or_else(|| {
            Error::InvalidArgument("knowledge-show id must be dom-<slug> or rb-<slug>".into())
        })?;
    let library = &call.ports.library;
    let pages = library.knowledge_pages()?;
    let (path, markdown) = pages
        .iter()
        .find_map(|file| {
            let markdown = file.markdown.as_deref()?;
            (DocumentRecord::from_markdown(Path::new(&file.path), markdown).id == id)
                .then_some((file.path.as_str(), markdown))
        })
        .ok_or_else(|| Error::PageNotFound { id: id.into() })?;
    let mut view = Page::from_markdown(markdown)?.model_view();
    view.path = path.to_string();
    let backlinks = library.backlinks(id)?;
    Ok(json!({
        "page": view,
        "backlinks": backlinks,
    }))
}
