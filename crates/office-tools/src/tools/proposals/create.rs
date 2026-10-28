//! File a proposed task, immutable reviewed payload and Library evidence page.

use crate::registry::{Call, Tool};
use office_core::domain::knowledge::{Page, PageFile};
use office_core::domain::proposal::{self, Create, PageDraft, Payload, RetireTarget};
use office_core::domain::source::{format_rfc3339_utc, sha256_hex};
use office_core::domain::template::{Template, TemplateFile};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "proposal-create",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["title", "description", "evidence", "payload", "tags"],
    run,
);

fn run(call: &Call<'_>) -> Result<Value> {
    let input: Create = serde_json::from_value(Value::Object(call.input.clone()))
        .map_err(|_| proposal::invalid("closed title, description, evidence, payload and tags"))?;
    // Conflicts describe the shared clone's state; only the sync files them.
    if input.payload.kind() == proposal::ProposalKind::SharedConflict {
        return Err(proposal::invalid(
            "payload kind (shared_conflict is filed by office.library-sync)",
        ));
    }
    file(call, input)
}

/// File one proposal: dismissal memory, staged payload, task, artifact and
/// evidence page. `office.library-sync` files through here too.
pub(crate) fn file(call: &Call<'_>, mut input: Create) -> Result<Value> {
    if input.title.trim().is_empty()
        || input.description.trim().is_empty()
        || input.tags.iter().any(|tag| {
            tag.trim().is_empty()
                || tag.contains(['\r', '\n'])
                || tag == "proposal"
                || tag.starts_with("proposal:")
                || tag.starts_with("proposal-cluster:")
        })
    {
        return Err(proposal::invalid("title, description and tags"));
    }
    let cluster = proposal::cluster(input.payload.kind(), &input.evidence)?;
    materialize(call, &mut input.payload, &input.evidence)?;
    input.payload.apply_sharing()?;
    validate_payload(call, &input.payload)?;
    let listed = call.ports.orbit.task_list(&object(json!({
        "tag":cluster,"status":"rejected","limit":1000,
        "fields":["id","status","tags","updated_at"]
    }))?)?;
    // Dismissal memory is an integrity read; missing or truncated data refuses.
    if listed.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(proposal::refused(
            "proposal_history_incomplete",
            "proposal dismissal history is missing or truncated",
        ));
    }
    let tasks = listed
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| proposal::invalid("dismissal tasks"))?;
    let now = office_core::domain::metrics::timestamp(&format_rfc3339_utc(call.ports.clock.now()))
        .ok_or_else(|| proposal::invalid("clock"))?;
    for task in tasks {
        if proposal::recently_dismissed(task, &cluster, now)? {
            return Ok(
                json!({"created":false,"id":task.get("id").and_then(Value::as_str).ok_or(Error::OrbitOutcomeUnknown)?,"suppressed":true,"evidence_page":null,"payload_sha256":null}),
            );
        }
    }
    let mut tags = vec![
        "proposal".to_string(),
        format!("proposal:{}", input.payload.kind().as_str()),
        cluster,
    ];
    tags.extend(input.tags);
    tags.sort();
    tags.dedup();
    let evidence = input
        .evidence
        .iter()
        .map(|id| format!("- {id}"))
        .collect::<Vec<_>>()
        .join("\n");
    let payload =
        serde_json::to_string_pretty(&input.payload).map_err(|_| proposal::invalid("payload"))?;
    // Stage first: no task is filed if staging fails. Never retry an uncertain
    // callback. Keep staged contents on failure so the original can be recovered.
    let staged = call
        .ports
        .library
        .stage_attachment("proposal.json", &payload)?;
    let task = call.ports.orbit.task_add(&object(json!({
        "title":input.title,"description":format!("{}\n\nEvidence:\n{evidence}\n\nPayload: proposal.json",input.description),
        "tags":tags,"type":"chore","origin":"automation","complexity":"low"
    }))?)?;
    if task
        .get("task")
        .unwrap_or(&task)
        .get("status")
        .and_then(Value::as_str)
        != Some("proposed")
    {
        return Err(Error::OrbitOutcomeUnknown);
    }
    let id = task
        .get("task")
        .unwrap_or(&task)
        .get("id")
        .and_then(Value::as_str)
        .ok_or(Error::OrbitOutcomeUnknown)?;
    // Orbit ids may include uppercase; prohibit traversal regardless of source.
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::OrbitOutcomeUnknown);
    }
    let path = format!("proposals/{id}.md");
    call.ports.orbit.task_artifact_put(&object(
        json!({"id":id,"source_path":staged,"path":"proposal.json"}),
    )?)?;
    call.ports.library.remove_staged(&staged)?;
    let markdown = format!(
        "---\nid: {id}\ntype: proposal\ntitle: {}\nkind: {}\ncreated_at: {}\n---\n{}\n\n## Evidence\n{evidence}\n\n## Reviewed payload\n```json\n{payload}\n```\n",
        json!(input.title),
        input.payload.kind().as_str(),
        format_rfc3339_utc(call.ports.clock.now()),
        input.description
    );
    call.ports
        .library
        .write_proposal(Path::new(&path), &markdown)?;
    Ok(
        json!({"created":true,"id":id,"suppressed":false,"evidence_page":path,"payload_sha256":sha256_hex(payload.as_bytes())}),
    )
}

pub(crate) fn object(value: Value) -> Result<Map<String, Value>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| proposal::invalid("mapping"))
}

fn materialize(call: &Call<'_>, payload: &mut Payload, evidence: &[String]) -> Result<()> {
    match payload {
        Payload::KnowledgeAdd { page, markdown, .. }
        | Payload::RunbookFromFriction { page, markdown } => {
            match (page.take(), markdown.as_ref()) {
                (Some(draft), None) => *markdown = Some(render(call, &draft, evidence)?),
                (None, Some(_)) => {}
                (Some(_), Some(_)) | (None, None) => {
                    return Err(proposal::invalid("exactly one of page or markdown"));
                }
            }
        }
        Payload::AutoTask { .. }
        | Payload::KnowledgeUpdate { .. }
        | Payload::Retire { .. }
        | Payload::SharedConflict { .. } => {}
    }
    Ok(())
}

fn render(call: &Call<'_>, draft: &PageDraft, evidence: &[String]) -> Result<String> {
    if draft.title.trim().is_empty() {
        return Err(proposal::invalid("page title"));
    }
    let mut cites = draft.cites.clone();
    cites.extend(
        evidence
            .iter()
            .filter(|e| {
                ["src-", "dom-", "rb-", "art-"]
                    .iter()
                    .any(|prefix| e.starts_with(prefix))
            })
            .cloned(),
    );
    cites.sort();
    cites.dedup();
    let (body, sensitivity) = if let Some(name) = &draft.template {
        if !draft.body.is_empty() {
            return Err(proposal::invalid("sections for a templated draft"));
        }
        let templates = call.ports.library.templates()?;
        let mut found = None;
        for file in &templates {
            let markdown = file
                .markdown
                .as_deref()
                .ok_or_else(|| proposal::invalid("readable template"))?;
            let template = Template::from_markdown(&file.path, markdown)?;
            if &template.template == name && found.is_none() {
                found = Some(template);
            }
        }
        let template = found.ok_or_else(|| Error::UnknownTemplate {
            id: draft.id.clone(),
            template: name.clone(),
        })?;
        if draft
            .sections
            .keys()
            .any(|key| !template.sections.contains(key))
        {
            return Err(proposal::invalid("template section names"));
        }
        let mut body = String::new();
        let mut replacement = false;
        for line in template.body.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                replacement = false;
                body.push_str(line);
                body.push('\n');
                if let Some(content) = draft.sections.get(heading) {
                    body.push_str(content);
                    body.push_str("\n\n");
                    replacement = true;
                }
            } else if !replacement {
                body.push_str(line);
                body.push('\n');
            }
        }
        (body, template.sensitivity.as_str())
    } else {
        if !draft.sections.is_empty() {
            return Err(proposal::invalid("template for sections"));
        }
        (draft.body.clone(), "normal")
    };
    let template = draft
        .template
        .as_ref()
        .map(|name| format!("template: {}\n", json!(name)))
        .unwrap_or_default();
    Ok(format!(
        "---\nid: {}\ntitle: {}\n{template}cites: {}\nshareable: {}\nsensitivity: {sensitivity}\n---\n{body}\n\nEvidence:\n{}\n",
        json!(draft.id),
        json!(draft.title),
        json!(cites),
        draft.shareable,
        evidence
            .iter()
            .map(|id| format!("- {id}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// All Library and payload prerequisites precede the acceptance reservation.
pub(crate) fn validate_payload(call: &Call<'_>, payload: &Payload) -> Result<()> {
    match payload {
        Payload::AutoTask { definition } => definition.validate(),
        Payload::KnowledgeAdd { page, markdown, .. }
        | Payload::RunbookFromFriction { page, markdown } => {
            if page.is_some() {
                return Err(proposal::invalid("materialized proposal page"));
            }
            let markdown = markdown
                .as_deref()
                .ok_or_else(|| proposal::invalid("page Markdown"))?;
            let parsed = validate_page(call, markdown)?;
            if payload.kind() == proposal::ProposalKind::RunbookFromFriction
                && parsed.kind != office_core::domain::knowledge::DocumentKind::Runbook
            {
                return Err(proposal::invalid("runbook page"));
            }
            if call.ports.library.get(&parsed.id)?.is_some() {
                return Err(proposal::refused(
                    "proposal_page_exists",
                    "proposal page already exists",
                ));
            }
            Ok(())
        }
        Payload::KnowledgeUpdate {
            markdown,
            base_sha256,
            ..
        } => {
            let page = validate_page(call, markdown)?;
            check_revision(call, &page.id, base_sha256).map(|_| ())
        }
        Payload::Retire {
            target: RetireTarget::AutoTask { name },
        } => {
            if proposal::valid_name(name) {
                Ok(())
            } else {
                Err(proposal::invalid("retirement name"))
            }
        }
        Payload::Retire {
            target: RetireTarget::Page { id, base_sha256 },
        } => {
            let text = check_revision(call, id, base_sha256)?;
            Page::from_markdown(&text)?;
            Ok(())
        }
        Payload::SharedConflict {
            page,
            path,
            blob,
            markdown,
            base_sha256,
        } => {
            use office_core::domain::sharing::sync::{SharedPath, classify};
            if classify(path) != (SharedPath::Page { id: page.clone() }) || blob.trim().is_empty() {
                return Err(proposal::invalid("shared page path and blob"));
            }
            if validate_page(call, markdown)?.id != *page {
                return Err(proposal::invalid("conflict page id"));
            }
            check_revision(call, page, base_sha256).map(|_| ())
        }
    }
}

pub(crate) fn check_revision(call: &Call<'_>, id: &str, base: &str) -> Result<String> {
    let record = call.ports.library.get(id)?.ok_or_else(|| {
        proposal::refused("proposal_page_missing", "reviewed proposal page is absent")
    })?;
    let text = call.ports.library.read_text(Path::new(&record.path))?;
    if sha256_hex(text.as_bytes()) != base {
        return Err(proposal::refused(
            "proposal_revision_changed",
            "proposal page changed since review; create a new proposal",
        ));
    }
    Ok(text)
}

pub(crate) fn validate_page(call: &Call<'_>, markdown: &str) -> Result<Page> {
    let page = Page::from_markdown(markdown)?;
    check_template(call, &page)?;
    office_core::domain::sharing::validate_sharing(markdown, |id| call.ports.library.get(id))?;
    page.check_base_rules()?;
    page.resolve_citations(|id| call.ports.library.get(id))?;
    Ok(page)
}

/// A page that names a template meets its current rules.
pub(crate) fn check_template(call: &Call<'_>, page: &Page) -> Result<()> {
    if page.template.is_some() {
        let templates = call.ports.library.templates()?;
        let files: Vec<_> = templates
            .iter()
            .map(|f| TemplateFile {
                path: &f.path,
                markdown: f.markdown.as_deref(),
            })
            .collect();
        let pages = call.ports.library.knowledge_pages()?;
        if pages.iter().any(|f| f.markdown.is_none()) {
            return Err(proposal::invalid("readable knowledge pages"));
        }
        let page_files: Vec<_> = pages
            .iter()
            .filter_map(|f| {
                f.markdown.as_deref().map(|markdown| PageFile {
                    path: &f.path,
                    markdown,
                })
            })
            .collect();
        page.check_template(&files, &page_files)?;
    }
    Ok(())
}
