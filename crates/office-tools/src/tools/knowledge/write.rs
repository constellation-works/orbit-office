//! `office.knowledge-write`: Create or update a page; validate frontmatter
//! and cites; commit.
//!
//! Every check runs before the Library port's one write, so a refused page
//! writes and commits nothing, and an accepted page commits once
//! (specs/library.md, "Knowledge pages"). Order: frontmatter, the page's
//! template (knowledge-templates.md), the base rules, then every citation.

use std::path::Path;

use office_core::domain::knowledge::{Page, PageFile};
use office_core::domain::template::TemplateFile;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "knowledge-write",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["markdown"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let markdown = call
        .input
        .get("markdown")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::InvalidArgument("knowledge-write markdown must be a string".into())
        })?;
    let library = &call.ports.library;
    let page = Page::from_markdown(markdown)?;
    if page.template.is_some() {
        let templates = library.templates()?;
        let template_files: Vec<TemplateFile<'_>> = templates
            .iter()
            .map(|file| TemplateFile {
                path: &file.path,
                markdown: file.markdown.as_deref(),
            })
            .collect();
        let pages = library.knowledge_pages()?;
        let page_files: Vec<PageFile<'_>> = pages
            .iter()
            .filter_map(|file| {
                file.markdown.as_deref().map(|markdown| PageFile {
                    path: &file.path,
                    markdown,
                })
            })
            .collect();
        page.check_template(&template_files, &page_files)?;
    }
    office_core::domain::sharing::validate_sharing(markdown, |id| library.get(id))?;
    page.check_base_rules()?;
    page.resolve_citations(|id| library.get(id))?;

    let created = library.get(&page.id)?.is_none();
    let record = library.write_document(Path::new(&page.path()), markdown)?;
    Ok(json!({
        "id": record.id,
        "type": page.kind.as_str(),
        "path": record.path,
        "created": created,
    }))
}
