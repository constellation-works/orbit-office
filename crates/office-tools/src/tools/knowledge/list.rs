//! List the complete page corpus and unfilled bundled singleton templates.
//!
//! This is an all-or-nothing read: malformed or unreadable pages/templates
//! refuse the request rather than imply that a singleton page is absent.

use std::collections::BTreeSet;

use office_core::domain::knowledge::{DocumentRecord, Page};
use office_core::domain::template::{BUNDLED, Template};
use office_core::ports::LibraryText;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "knowledge-list",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &["template"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let filter = match call.input.get("template") {
        None => None,
        Some(Value::String(name))
            if name.split('-').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            }) =>
        {
            Some(name.as_str())
        }
        Some(_) => {
            return Err(Error::InvalidArgument(
                "knowledge-list template must be a lowercase kebab-case name".into(),
            ));
        }
    };
    let library = &call.ports.library;
    let mut pages = library
        .knowledge_pages()?
        .iter()
        .map(|file| {
            let text = markdown(file)?;
            let record = DocumentRecord::from_markdown(std::path::Path::new(&file.path), text);
            Ok((Page::from_markdown(text)?.model_view(), record))
        })
        .collect::<Result<Vec<_>>>()?;
    pages.sort_by(|a, b| a.0.id.cmp(&b.0.id).then_with(|| a.1.path.cmp(&b.1.path)));
    // Presence is computed on the full corpus, before the caller's filter.
    let present: BTreeSet<_> = pages
        .iter()
        .filter_map(|(p, _)| p.template.as_deref())
        .collect();
    let mut missing = Vec::new();
    for file in library.templates()? {
        if !BUNDLED
            .iter()
            .any(|bundled| file.path == format!(".office/templates/{}", bundled.filename))
        {
            continue;
        }
        let template = Template::from_markdown(&file.path, markdown(&file)?)?;
        if template.one_per_library
            && !present.contains(template.template.as_str())
            && filter.is_none_or(|name| name == template.template)
        {
            missing.push(template);
        }
    }
    missing.sort_by(|a, b| a.template.cmp(&b.template));
    let missing: Vec<_> = missing
        .into_iter()
        .map(|t| json!({"template":t.template,"id":t.id.as_str(),"title":t.title}))
        .collect();
    let pages: Vec<_> = pages
        .iter()
        .filter(|(p, _)| filter.is_none_or(|name| p.template.as_deref() == Some(name)))
        .map(|(p, record)| {
            json!({
                "id": p.id,
                "layer": record.layer,
                "shared_by": record.shared_by,
                "template": p.template,
                "title": p.title,
                "summary": p.summary,
                "status": p.status,
                "sensitivity": p.sensitivity,
                "shareable": record.shareable,
            })
        })
        .collect();
    Ok(json!({"pages": pages, "missing_templates": missing}))
}

fn markdown(file: &LibraryText) -> Result<&str> {
    file.markdown.as_deref().ok_or_else(|| Error::Library {
        code: "library_document_unreadable",
        message: format!("cannot read {}", file.path),
    })
}
