//! App-only approval: prepare, reserve once, apply and record the change.
use super::create::{check_revision, object, validate_payload};
use crate::registry::{Call, Tool};
use office_core::domain::artifact::decode_payload;
use office_core::domain::knowledge::Page;
use office_core::domain::proposal::{self, Payload, RetireTarget};
use office_core::domain::script::ScriptRequest;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "proposal-accept",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["id", "sha256"],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let id = call
        .input
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| proposal::invalid("proposal id"))?;
    let digest = call
        .input
        .get("sha256")
        .and_then(Value::as_str)
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or_else(|| proposal::invalid("reviewed payload sha256"))?;
    proposal::validate_record_path(Path::new(&format!("proposals/{id}.md")))?;
    let task = call
        .ports
        .orbit
        .task_show(&object(json!({"id":id,"fields":["id","status","tags"]}))?)?;
    let task = task.get("task").unwrap_or(&task);
    if task.get("status").and_then(Value::as_str) != Some("proposed") {
        return Err(proposal::refused(
            "proposal_closed",
            "proposal is not proposed; inspect its recorded outcome before retrying",
        ));
    }
    let tags = task
        .get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| proposal::invalid("proposal tags"))?;
    if !tags.contains(&json!("proposal")) {
        return Err(proposal::invalid("proposal task tag"));
    }
    let artifact = call
        .ports
        .orbit
        .task_artifact_get(&object(json!({"id":id,"path":"proposal.json"}))?)?;
    let bytes = decode_payload(&artifact)?;
    if office_core::domain::source::sha256_hex(&bytes) != digest {
        return Err(proposal::refused(
            "proposal_payload_changed",
            "proposal payload differs from the reviewed digest; review it again",
        ));
    }
    let mut payload: Payload = serde_json::from_slice(&bytes)
        .map_err(|_| proposal::invalid("closed proposal.json payload"))?;
    if !tags.contains(&json!(format!("proposal:{}", payload.kind().as_str()))) {
        return Err(proposal::invalid("matching proposal kind tag"));
    }
    payload.apply_sharing()?;
    validate_payload(call, &payload)?;
    if let Payload::AutoTask { definition } = &payload {
        for step in &definition.steps {
            if step.tool == "office.script-run" {
                let request: ScriptRequest =
                    serde_json::from_value(Value::Object(step.input.clone()))
                        .map_err(|_| proposal::invalid("pinned script input"))?;
                crate::prepare_script(call.ports, &request)?;
            }
        }
    }
    // Orbit's start transition checks the pickup state atomically. A concurrent
    // accept cannot start an in-progress proposal. No effect is retried after
    // an uncertain callback, including after a later completion failure.
    let reserved = call.ports.orbit.task_update(&object(json!({"id":id,"status":"in-progress",
        "plan":"Apply the user-reviewed proposal.json, record its change, and close through review.",
        "comment":format!("User accepted Office proposal payload sha256: {digest}")}))?)?;
    if reserved
        .get("task")
        .unwrap_or(&reserved)
        .get("status")
        .and_then(Value::as_str)
        != Some("in-progress")
    {
        return Err(Error::OrbitOutcomeUnknown);
    }
    let change = apply(call, &payload)?;
    let summary = format!(
        "{}{digest}\nUser accepted Office {} proposal. Applied change: {}",
        proposal::APPROVAL_DIGEST_PREFIX,
        payload.kind().as_str(),
        change
    );
    // Every Library write commits through the port. The receipt is committed
    // too, including for auto-task-only changes.
    call.ports.library.write_proposal(
        Path::new(&format!("proposals/{id}-accepted.md")),
        &format!("# Accepted proposal\n\n{summary}\n"),
    )?;
    let review = call.ports.orbit.task_update(&object(
        json!({"id":id,"status":"review","execution_summary":summary}),
    )?)?;
    if review
        .get("task")
        .unwrap_or(&review)
        .get("status")
        .and_then(Value::as_str)
        != Some("review")
    {
        return Err(Error::OrbitOutcomeUnknown);
    }
    let done = call.ports.orbit.task_update(&object(
        json!({"id":id,"status":"done","execution_summary":summary}),
    )?)?;
    if done
        .get("task")
        .unwrap_or(&done)
        .get("status")
        .and_then(Value::as_str)
        != Some("done")
    {
        return Err(Error::OrbitOutcomeUnknown);
    }
    Ok(json!({"id":id,"accepted":true,"change":change}))
}

fn apply(call: &Call<'_>, payload: &Payload) -> Result<Value> {
    match payload {
        Payload::AutoTask { definition } => {
            let added = call.ports.orbit.auto_task_add(&definition.callback()?)?;
            let record = added.get("auto_task").unwrap_or(&added);
            if record.get("name").and_then(Value::as_str) != Some(&definition.name) {
                return Err(Error::OrbitOutcomeUnknown);
            }
            let enabled = call
                .ports
                .orbit
                .auto_task_update(&object(json!({"name":definition.name,"enabled":true}))?)?;
            let record = enabled.get("auto_task").unwrap_or(&enabled);
            if record.get("name").and_then(Value::as_str) != Some(&definition.name)
                || record.get("enabled").and_then(Value::as_bool) != Some(true)
            {
                return Err(Error::OrbitOutcomeUnknown);
            }
            Ok(json!({"auto_task":definition.name,"enabled":true}))
        }
        Payload::KnowledgeAdd { markdown, .. } | Payload::RunbookFromFriction { markdown, .. } => {
            write_page(
                call,
                markdown
                    .as_deref()
                    .ok_or_else(|| proposal::invalid("materialized Markdown"))?,
                None,
            )
        }
        Payload::KnowledgeUpdate {
            markdown,
            base_sha256,
            ..
        } => {
            let page = Page::from_markdown(markdown)?;
            check_revision(call, &page.id, base_sha256)?;
            write_page(call, markdown, Some(base_sha256))
        }
        Payload::Retire {
            target: RetireTarget::AutoTask { name },
        } => {
            let disabled = call
                .ports
                .orbit
                .auto_task_update(&object(json!({"name":name,"enabled":false}))?)?;
            let record = disabled.get("auto_task").unwrap_or(&disabled);
            if record.get("name").and_then(Value::as_str) != Some(name)
                || record.get("enabled").and_then(Value::as_bool) != Some(false)
            {
                return Err(Error::OrbitOutcomeUnknown);
            }
            Ok(json!({"auto_task":name,"enabled":false}))
        }
        Payload::Retire {
            target: RetireTarget::Page { id, base_sha256 },
        } => {
            let text = check_revision(call, id, base_sha256)?;
            let newline = if text.starts_with("---\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let rest = text
                .strip_prefix(&format!("---{newline}"))
                .ok_or_else(|| proposal::invalid("page frontmatter"))?;
            let (front, body) = rest
                .split_once(&format!("{newline}---"))
                .ok_or_else(|| proposal::invalid("page frontmatter"))?;
            let front = front
                .lines()
                .filter(|line| !line.starts_with("status:"))
                .collect::<Vec<_>>()
                .join(newline);
            write_page(
                call,
                &format!("---{newline}{front}{newline}status: retired{newline}---{body}"),
                Some(base_sha256),
            )
        }
    }
}
fn write_page(call: &Call<'_>, markdown: &str, expected: Option<&str>) -> Result<Value> {
    let page = Page::from_markdown(markdown)?;
    let path = if expected.is_some() {
        call.ports
            .library
            .get(&page.id)?
            .ok_or_else(|| proposal::refused("proposal_page_missing", "reviewed page is absent"))?
            .path
    } else {
        page.path()
    };
    let record = call
        .ports
        .library
        .write_reviewed_page(Path::new(&path), markdown, expected)?;
    Ok(json!({"page":record.id,"path":record.path}))
}
