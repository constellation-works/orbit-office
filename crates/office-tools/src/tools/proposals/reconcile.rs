//! File each proposal page under the status folder its Orbit task says.
//!
//! Acceptance moves its own page; this catches pages whose task closed
//! elsewhere (a dismissal, an operator's CLI) and migrates legacy flat pages.
use super::create::object;
use crate::registry::{Call, Tool};
use office_core::Result;
use office_core::domain::proposal::{
    self, ProposalStatus, RecordLocation, RecordPath, parse_record_path, record_path, with_outcome,
};
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "proposal-reconcile",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &[],
    run,
);

/// Suffix of the separate acceptance receipt Office wrote before pages moved.
const LEGACY_RECEIPT: &str = "-accepted";

/// One proposal page found in the Library.
pub(crate) struct ProposalPage {
    path: String,
    record: RecordPath,
    markdown: Option<String>,
    receipt: bool,
}

fn run(call: &Call<'_>) -> Result<Value> {
    let listed = call.ports.orbit.task_list(&object(json!({
        "tag":"proposal","limit":1000,"fields":["id","status","updated_at"]
    }))?)?;
    // A page whose task was omitted would look unknown; refuse a partial list.
    if listed.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(proposal::refused(
            "proposal_history_incomplete",
            "proposal task list is missing or truncated",
        ));
    }
    let mut tasks = BTreeMap::new();
    for task in listed
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| proposal::invalid("proposal tasks"))?
    {
        let id = task
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| proposal::invalid("proposal task id"))?;
        let status = task
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| proposal::invalid("proposal task status"))?;
        let updated = task.get("updated_at").and_then(Value::as_str);
        tasks.insert(
            id.to_string(),
            (status.to_string(), updated.map(str::to_string)),
        );
    }
    let mut groups: BTreeMap<String, Vec<ProposalPage>> = BTreeMap::new();
    for page in pages(call)? {
        let id = owner(&page, |id| tasks.contains_key(id));
        groups.entry(id).or_default().push(page);
    }
    let mut moved = Vec::new();
    let mut unknown = Vec::new();
    for (id, pages) in groups {
        let Some((status, updated)) = tasks.get(&id) else {
            unknown.push(id);
            continue;
        };
        let target = match ProposalStatus::from_orbit(status) {
            Some(target) => target,
            // A task between statuses keeps its page; only a legacy flat page
            // moves, and only to open/ until the task closes.
            None if pages
                .iter()
                .any(|page| page.record.location == RecordLocation::Flat) =>
            {
                ProposalStatus::Open
            }
            None => continue,
        };
        let closed_at = updated
            .as_deref()
            .filter(|at| proposal::closed_month(at).is_some())
            .map_or_else(
                || format_rfc3339_utc(call.ports.clock.now()),
                str::to_string,
            );
        let outcome = match target {
            ProposalStatus::Open => None,
            ProposalStatus::Accepted => Some(format!(
                "Accepted: the Orbit task is `{status}` (updated {closed_at}).{}",
                receipts(&pages)
            )),
            ProposalStatus::Dismissed => Some(format!(
                "Dismissed: the Orbit task is `{status}` (updated {closed_at}). The task \
                 history holds the reason."
            )),
        };
        if let Some(change) = file(call, &id, target, &closed_at, outcome.as_deref(), &pages)? {
            moved.push(change);
        }
    }
    Ok(json!({"moved":moved,"unknown":unknown}))
}

/// Every proposal page, refusing one that cannot be read.
fn pages(call: &Call<'_>) -> Result<Vec<ProposalPage>> {
    call.ports
        .library
        .proposal_pages()?
        .into_iter()
        .map(|file| {
            let record = parse_record_path(Path::new(&file.path))?;
            let receipt = record.location == RecordLocation::Flat
                && record.id.ends_with(LEGACY_RECEIPT)
                && file
                    .markdown
                    .as_deref()
                    .is_some_and(|text| text.starts_with("# Accepted proposal"));
            Ok(ProposalPage {
                path: file.path,
                record,
                markdown: file.markdown,
                receipt,
            })
        })
        .collect()
}

/// The proposal a page belongs to: its stem, or for a legacy acceptance
/// receipt `<id>-accepted.md`, the proposal it records.
fn owner(page: &ProposalPage, known: impl Fn(&str) -> bool) -> String {
    if page.receipt
        && !known(&page.record.id)
        && let Some(id) = page.record.id.strip_suffix(LEGACY_RECEIPT)
    {
        return id.to_string();
    }
    page.record.id.clone()
}

/// Every page of proposal `id`, in any status folder or the legacy layout.
pub(crate) fn pages_for(call: &Call<'_>, id: &str) -> Result<Vec<ProposalPage>> {
    Ok(pages(call)?
        .into_iter()
        .filter(|page| owner(page, |_| false) == id)
        .collect())
}

/// Legacy receipts' text, folded into the outcome when their page moves.
fn receipts(pages: &[ProposalPage]) -> String {
    pages
        .iter()
        .filter(|page| page.receipt)
        .filter_map(|page| page.markdown.as_deref())
        .map(|text| {
            format!(
                "\n\n{}",
                text.trim_start_matches("# Accepted proposal").trim()
            )
        })
        .collect()
}

/// File proposal `id`'s page under `target` in one Library commit: the page
/// moves from wherever it is, and a closed page gains `outcome`. A page that
/// is already the only page in its target folder is left alone, so a re-run
/// changes nothing. Returns the move, or `None` when nothing changed.
pub(crate) fn file(
    call: &Call<'_>,
    id: &str,
    target: ProposalStatus,
    closed_at: &str,
    outcome: Option<&str>,
    pages: &[ProposalPage],
) -> Result<Option<Value>> {
    let current = pages
        .iter()
        .find(|page| !page.receipt && page.record.location.status() == Some(target));
    let to = match current {
        Some(page) => page.path.clone(),
        None => record_path(id, target, closed_at)?,
    };
    if pages.len() == 1 && pages[0].path == to {
        return Ok(None);
    }
    // The evidence page itself: the one already in place, else the newest
    // layout it was filed under.
    let base = current
        .or_else(|| {
            pages
                .iter()
                .find(|page| page.record.location == RecordLocation::Open)
        })
        .or_else(|| {
            pages
                .iter()
                .find(|page| !page.receipt && page.record.location == RecordLocation::Flat)
        })
        .or_else(|| pages.iter().find(|page| !page.receipt));
    let page = match base {
        Some(page) => page.markdown.clone().ok_or_else(|| {
            proposal::refused("proposal_page_unreadable", "a proposal page is unreadable")
        })?,
        None => format!("---\nid: {id}\ntype: proposal\n---\n"),
    };
    let text = match outcome {
        Some(outcome) => with_outcome(&page, outcome),
        None => page,
    };
    let from: Vec<String> = pages.iter().map(|page| page.path.clone()).collect();
    call.ports
        .library
        .move_proposal(Path::new(&to), &text, &from)?;
    Ok(Some(json!({
        "id": id,
        "from": from.into_iter().filter(|path| *path != to).collect::<Vec<_>>(),
        "to": to,
    })))
}

/// The acceptance outcome's closing time, from the call's clock.
pub(crate) fn now(call: &Call<'_>) -> String {
    format_rfc3339_utc(call.ports.clock.now())
}
