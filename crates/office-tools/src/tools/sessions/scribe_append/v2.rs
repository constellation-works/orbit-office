//! The v2 pass of `office.scribe-append` (specs/scribe.md, "Pipeline"
//! step 4). Model-free: the markers are recomputed from the parse
//! `office.scribe-pending` wrote, every finding is checked against them,
//! and the SCR, the ledger lines and any apply-mode page land in one
//! Library commit.

use std::path::Path;

use office_core::domain::knowledge::DocumentRecord;
use office_core::domain::source::conversation::scribe::{self, Parsed, v2};
use office_core::domain::source::decisions::Ledger;
use office_core::domain::source::format_rfc3339_utc;
use office_core::ports::{ScribeChain, ScribePass, ScribeWrite};
use office_core::{Error, Result};
use serde_json::{Value, json};

use super::{not_parsed, optional};
use crate::registry::Call;

pub(super) fn run(call: &Call<'_>, session_id: &str) -> Result<Value> {
    for key in ["entries", "counterpart", "counterpart_name"] {
        if call.input.get(key).is_some_and(|v| !v.is_null()) {
            return Err(Error::InvalidArgument(format!(
                "scribe-append {key} is a v1 input; a v2 pass takes sections and findings"
            )));
        }
    }
    let through = optional(call, "through")?
        .ok_or_else(|| Error::InvalidArgument("scribe-append requires through".into()))?;
    let sections = call
        .input
        .get("sections")
        .filter(|v| !v.is_null())
        .ok_or_else(|| Error::InvalidArgument("scribe-append v2 requires sections".into()))?;
    let library = call.ports.library.as_ref();
    let parse = library
        .read_scribe_parse(session_id)?
        .ok_or_else(not_parsed)?;
    let messages = scribe::from_jsonl(&parse.messages)?;
    let parsed: Parsed = serde_json::from_str(&parse.meta).map_err(|_| not_parsed())?;
    if parsed.session_id != session_id || parsed.scope.is_none() {
        return Err(not_parsed());
    }
    let tags = library.settings()?.scribe.effective_tags();
    let files = v2::v2_files(&library.scribe_files()?)?;
    let mut ledgers = Vec::new();
    for file in library.decision_ledgers()? {
        let text = file.markdown.unwrap_or_default();
        ledgers.push((Ledger::parse(&file.path, &text)?, text));
    }
    let pages = library.knowledge_pages()?;
    let now = format_rfc3339_utc(call.ports.clock.now());
    let zone = call.ports.clock.time_zone();
    let request = v2::PassRequest {
        session_id,
        through,
        title: optional(call, "title")?,
        title_by: optional(call, "title_by")?,
        sections,
        findings: call.input.get("findings"),
        now: &now,
        zone: zone.as_deref(),
    };
    let stores = v2::PassStores {
        messages: &messages,
        parsed: &parsed,
        tags: &tags,
        files: &files,
        ledgers: &ledgers,
        pages: &pages,
    };
    // An apply-mode page passes `office.knowledge-write`'s validation, with
    // its `<SCR>#T<n>` citations resolved against the SCR this pass writes.
    let mut validate_page = |page: &str, scr_markdown: &str| -> bool {
        let Ok(scr) = v2::Scr::parse("", scr_markdown) else {
            return false;
        };
        let path = format!("{}/{}.md", scribe::SCR_DIR, scr.front.id);
        let record = DocumentRecord::from_markdown(Path::new(&path), scr_markdown);
        let get = |id: &str| {
            if id == record.id {
                Ok(Some(record.clone()))
            } else {
                library.get(id)
            }
        };
        crate::tools::knowledge::write::validate_with(library, page, &get)
            .is_ok_and(|page| page.template.as_deref() == Some("preferences"))
    };
    let plan = v2::plan_pass(request, stores, &mut validate_page)?;
    let created = plan.current.is_none();
    if let Some(markdown) = &plan.markdown {
        library.write_scribe_pass(&ScribePass {
            scr: ScribeWrite {
                path: plan.path.clone(),
                text: markdown.clone(),
                previous: plan.current.clone().map(|text| (plan.path.clone(), text)),
            },
            files: plan.files.clone(),
            chain: created.then(|| ScribeChain {
                scope: plan.scope.clone(),
                session: session_id.to_owned(),
                previous: plan.front.previous.clone(),
            }),
        })?;
    }
    let written = plan.markdown.is_some();
    Ok(json!({
        "format": "v2",
        "session_id": session_id,
        "id": plan.id,
        "path": plan.path,
        "created": created && written,
        "written": written,
        "title": plan.front.title,
        "title_by": plan.front.title_by,
        "scope": plan.scope,
        "previous": plan.front.previous,
        "scribed_through": if written { plan.front.scribed_through.clone() } else {
            files.iter().find(|f| f.front.session == session_id).and_then(|f| f.front.scribed_through.clone())
        },
        "added": plan.added,
        "ledger": plan.ledger,
        "applied": plan.applied,
        "needs_proposal": plan.needs_proposal,
        "withheld": plan.withheld,
        "reported": plan.reported,
        "duplicates": plan.duplicates,
        "refused": plan.refused,
        "files": plan.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
    }))
}
