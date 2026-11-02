//! `office.scribe-migrate`: the one-time scribe v1 tidy-up
//! (specs/scribe.md, "Migration").
//!
//! Scribe v2 starts clean: v1 preferences and decisions are not backfilled
//! (Daniel, 2026-11-02). This tool only:
//!
//! - files one `knowledge_update` removing the v1 Tags section and the
//!   `scribe:` mapping from `dom-scribe-rules`, when the page still has
//!   either, unless that proposal was already filed in any status;
//! - reports the open v1 `#pending` lines for the user to turn into
//!   to-dos, never filing them. Private SCR files and a private
//!   conversation setting are skipped.

use std::collections::BTreeMap;
use std::path::Path;

use office_core::domain::knowledge::SCRIBE_RULES_PAGE;
use office_core::domain::proposal::{self, Create, Payload};
use office_core::domain::source::conversation::scribe;
use office_core::domain::source::{sha256_hex, source_body};
use office_core::ports::Sensitivity;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use super::scribe_query::{Entry, entries_of};
use crate::registry::{Call, Tool};
use crate::tools::proposals::create::{self, object};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "scribe-migrate",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["dry_run"],
    run,
);

/// The tag on every proposal this migration files.
const TAG: &str = "scribe-migration";

/// One filing the migration plans.
struct Planned {
    key: String,
    kind: &'static str,
    create: Create,
}

fn invalid(message: &str) -> Error {
    Error::InvalidArgument(format!("scribe-migrate {message}"))
}

fn run(call: &Call<'_>) -> Result<Value> {
    let dry_run = match call.input.get("dry_run") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(invalid("dry_run must be a boolean")),
    };
    let settings = call.ports.library.settings()?;
    let conversation_private = settings
        .sensitivity
        .get("conversation")
        .is_some_and(|s| *s == Sensitivity::Private);

    let files = scribe::scr_files(&call.ports.library.scribe_files()?)?;
    let entries = entries_of(&files);
    // A v1 line is numbered in its file; a citation counts body lines.
    let offsets: BTreeMap<String, usize> = files
        .iter()
        .map(|file| {
            let total = file.markdown.lines().count();
            let body = source_body(&file.markdown).lines().count();
            (file.path.clone(), total.saturating_sub(body))
        })
        .collect();
    let mut skipped_private = 0usize;
    let pending = open_pending(
        &entries,
        &offsets,
        conversation_private,
        &mut skipped_private,
    );

    let planned: Vec<Planned> = rules_plan(call)?.into_iter().collect();
    let existing = if planned.is_empty() {
        BTreeMap::new()
    } else {
        filed_keys(call)?
    };
    let mut proposals = Vec::new();
    for mut plan in planned {
        let key_tag = format!("{TAG}:{}", &sha256_hex(plan.key.as_bytes())[..16]);
        let (status, id) = if let Some(id) = existing.get(&key_tag) {
            ("already_filed", Some(id.clone()))
        } else if dry_run {
            ("planned", None)
        } else {
            plan.create.tags = vec![TAG.to_owned(), key_tag];
            let filed = create::file(call, plan.create)?;
            let id = filed.get("id").and_then(Value::as_str).map(str::to_owned);
            if filed.get("suppressed").and_then(Value::as_bool) == Some(true) {
                ("suppressed", id)
            } else {
                ("filed", id)
            }
        };
        proposals.push(json!({"key": plan.key, "kind": plan.kind, "status": status, "id": id}));
    }
    Ok(json!({
        "dry_run": dry_run,
        "proposals": proposals,
        "pending": pending,
        "skipped_private": skipped_private,
    }))
}

/// The open v1 `#pending` lines: those no `#resolved` line names.
fn open_pending(
    entries: &[Entry],
    offsets: &BTreeMap<String, usize>,
    conversation_private: bool,
    skipped_private: &mut usize,
) -> Vec<Value> {
    let resolved: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.tag == "resolved")
        .filter_map(|entry| entry.resolves.as_deref())
        .collect();
    let mut pending = Vec::new();
    for entry in entries.iter().filter(|entry| entry.tag == "pending") {
        if entry.private || conversation_private {
            *skipped_private += 1;
            continue;
        }
        let quote = entry.quote.trim();
        if quote.is_empty()
            || entry
                .keys()
                .iter()
                .any(|key| resolved.contains(&key.as_str()))
        {
            continue;
        }
        let offset = offsets.get(&entry.path).copied().unwrap_or_default();
        pending.push(json!({
            "cite": format!("{}#L{}", entry.scr_id, entry.line.saturating_sub(offset)),
            "quote": quote,
            "time": entry.time,
            "session_id": entry.session_id,
            "due": entry.due,
        }));
    }
    pending
}

/// The rules-page proposal: drop the v1 Tags section and the retired
/// `scribe:` front matter mapping, when the page has either.
fn rules_plan(call: &Call<'_>) -> Result<Option<Planned>> {
    let Some(record) = call.ports.library.get(SCRIBE_RULES_PAGE)? else {
        return Ok(None);
    };
    let text = call.ports.library.read_text(Path::new(&record.path))?;
    let Some(markdown) = without_v1_rules(&text) else {
        return Ok(None);
    };
    Ok(Some(Planned {
        key: "rules".to_owned(),
        kind: "knowledge_update",
        create: Create {
            title: "Remove the scribe v1 Tags section from the scribe rules".to_owned(),
            description: format!(
                "Scribe v2 migration: scribe v2 reads tag definitions from the scribe settings, so the Tags section of {SCRIBE_RULES_PAGE} is no longer read, and the scribe handoff flag is retired. Accepting removes the Tags section and the scribe: front matter mapping; Topics, Matters, Capture and Models stay."
            ),
            evidence: vec![SCRIBE_RULES_PAGE.to_owned()],
            payload: Payload::KnowledgeUpdate {
                conflicts: Vec::new(),
                shareable: None,
                markdown,
                base_sha256: sha256_hex(text.as_bytes()),
            },
            tags: Vec::new(),
        },
    }))
}

/// The rules page without its `## Tags` section and `scribe:` mapping, or
/// `None` when it has neither.
fn without_v1_rules(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let (front, body) = rest.split_once("\n---\n")?;
    let mut changed = false;
    let mut kept_front = Vec::new();
    let mut in_scribe = false;
    for line in front.lines() {
        if line.trim_end() == "scribe:" || line.starts_with("scribe:") {
            in_scribe = true;
            changed = true;
            continue;
        }
        if in_scribe && (line.starts_with(' ') || line.starts_with('\t')) {
            continue;
        }
        in_scribe = false;
        kept_front.push(line);
    }
    let mut kept_body = String::new();
    let mut in_tags = false;
    for line in body.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            in_tags = heading.trim() == "Tags";
            changed |= in_tags;
        }
        if !in_tags {
            kept_body.push_str(line);
        }
    }
    changed.then(|| format!("---\n{}\n---\n{kept_body}", kept_front.join("\n")))
}

/// The migration's filed keys: per-key tag to task id, in any status.
fn filed_keys(call: &Call<'_>) -> Result<BTreeMap<String, String>> {
    let listed = call.ports.orbit.task_list(&object(json!({
        "tag": TAG, "limit": 1000, "fields": ["id", "status", "tags"]
    }))?)?;
    // A truncated or missing list could re-file a key: refuse instead.
    if listed.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(proposal::refused(
            "proposal_history_incomplete",
            "the migration's proposal history is missing or truncated",
        ));
    }
    let mut out = BTreeMap::new();
    for task in listed
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| proposal::invalid("migration tasks"))?
    {
        let Some(id) = task.get("id").and_then(Value::as_str) else {
            continue;
        };
        for tag in task
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|tag| tag.starts_with(&format!("{TAG}:")))
        {
            out.insert(tag.to_owned(), id.to_owned());
        }
    }
    Ok(out)
}
