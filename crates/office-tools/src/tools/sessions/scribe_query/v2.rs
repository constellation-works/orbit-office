//! The scribe v2 modes of `office.scribe-query` (specs/scribe.md,
//! "Answering"): `decisions` over the decision ledgers, `preferences` over
//! the preference pages, and `scr` over the v2 SCR files. Model-free and
//! read-only; v1 modes are unchanged.

use office_core::domain::knowledge::{Page, preferences};
use office_core::domain::source::conversation::scribe::v2::{self, Scr};
use office_core::domain::source::decisions::{DecidedBy, Ledger, Scope, views};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use super::{optional_date, optional_text};
use crate::registry::Call;

/// The v2 modes.
pub(super) const MODES: &[&str] = &["decisions", "preferences", "scr"];

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

/// The `scope` filter (`global`, `project:<name>`), or `project` as its
/// shorthand.
fn scope_filter(input: &Map<String, Value>) -> Result<Option<String>> {
    match (
        optional_text(input, "scope")?,
        optional_text(input, "project")?,
    ) {
        (Some(scope), _) => Ok(Some(Scope::parse(&scope)?.token())),
        (None, Some(project)) => Ok(Some(Scope::parse(&format!("project:{project}"))?.token())),
        (None, None) => Ok(None),
    }
}

pub(super) fn run(call: &Call<'_>, mode: &str, today: &str, private: bool) -> Result<Value> {
    let input = call.input;
    for key in ["tag", "topic", "refs", "session_id"] {
        if input.get(key).is_some_and(|v| !v.is_null()) {
            return Err(invalid(format!(
                "scribe-query {key} filters the v1 modes, not {mode}"
            )));
        }
    }
    match mode {
        "decisions" => decisions(call, input),
        "preferences" => preferences_mode(call, input, today),
        _ => scr(call, input, private),
    }
}

fn decisions(call: &Call<'_>, input: &Map<String, Value>) -> Result<Value> {
    let scope = scope_filter(input)?;
    let decided_by = match optional_text(input, "decided_by")?.as_deref() {
        None => None,
        Some("human") => Some(DecidedBy::Human),
        Some("agent") => Some(DecidedBy::Agent),
        Some("model") => Some(DecidedBy::Model),
        Some(_) => return Err(invalid("scribe-query decided_by is human, agent or model")),
    };
    let since = optional_date(input, "since")?;
    let until = optional_date(input, "until")?;
    let text = optional_text(input, "text")?.map(|t| t.to_lowercase());
    let task = optional_text(input, "task")?;
    let workspace = optional_text(input, "workspace")?;
    let mut ledgers = Vec::new();
    for file in call.ports.library.decision_ledgers()? {
        ledgers.push(Ledger::parse(
            &file.path,
            file.markdown.as_deref().unwrap_or_default(),
        )?);
    }
    let all = views(&ledgers);
    let total_entries = all.len();
    let results: Vec<Value> = all
        .into_iter()
        .filter(|view| {
            let d = &view.decision;
            let date = d.decided_at.get(..10).unwrap_or_default();
            let haystack = [
                d.question.as_deref(),
                d.quote.as_deref(),
                d.chosen.as_deref(),
                d.because.as_ref().map(|b| b.text.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
            scope.as_deref().is_none_or(|s| d.scope == s)
                && decided_by.is_none_or(|by| d.decided_by == by)
                && since.as_deref().is_none_or(|s| date >= s)
                && until.as_deref().is_none_or(|u| date <= u)
                && text.as_deref().is_none_or(|t| haystack.contains(t))
                && task
                    .as_deref()
                    .is_none_or(|t| d.context.task.as_deref() == Some(t))
                && workspace
                    .as_deref()
                    .is_none_or(|w| d.context.workspace.as_deref() == Some(w))
        })
        .map(|view| {
            let current = view.reversed_by.is_none();
            let mut value = json!(view);
            value["current"] = Value::Bool(current);
            value
        })
        .collect();
    Ok(json!({
        "mode": "decisions",
        "decisions": results,
        "count": results.len(),
        "ledgers": ledgers.iter().map(Ledger::id).collect::<Vec<_>>(),
        "entries_searched": total_entries,
    }))
}

fn preferences_mode(call: &Call<'_>, input: &Map<String, Value>, today: &str) -> Result<Value> {
    let scope = scope_filter(input)?;
    let matter = optional_text(input, "matter")?;
    let library = call.ports.library.as_ref();
    let files = v2::v2_files(&library.scribe_files()?)?;
    let mut sections = Vec::new();
    for page in library.knowledge_pages()? {
        let Some(slug) = page
            .path
            .strip_prefix(v2::PREFERENCE_DIR)
            .and_then(|p| p.strip_prefix("/dom-preferences-"))
            .and_then(|p| p.strip_suffix(".md"))
        else {
            continue;
        };
        let page_scope = if slug == "global" {
            "global".to_owned()
        } else {
            format!("project:{slug}")
        };
        if scope.as_deref().is_some_and(|s| s != page_scope) {
            continue;
        }
        let Some(markdown) = page.markdown.as_deref() else {
            continue;
        };
        let parsed = Page::from_markdown(markdown)?;
        for record in preferences::parse(&parsed.id, &parsed.body)? {
            if matter.as_deref().is_some_and(|m| m != record.matter) {
                continue;
            }
            // Proposals SCR entries recorded for this scope and matter.
            let proposed: Vec<Value> = files
                .iter()
                .flat_map(|scr| {
                    scr.entries().into_iter().filter_map(|entry| {
                        (entry.scope == page_scope
                            && entry.field("matter") == Some(record.matter.as_str()))
                        .then(|| entry.field("proposal").map(str::to_owned))
                        .flatten()
                        .map(|proposal| {
                            json!({"entry": format!("{}#{}", scr.front.id, entry.id()),
                                "proposal": proposal})
                        })
                    })
                })
                .collect();
            sections.push(json!({
                "page": parsed.id, "scope": page_scope, "matter": record.matter,
                "citation": format!("{}#{}", parsed.id, record.matter),
                "statement": record.statement, "cites": record.cites,
                "stated_at": record.stated_at, "review_by": record.review_by,
                "stale": record.review_by.as_str() < today,
                "proposed": proposed,
            }));
        }
    }
    Ok(
        json!({"mode": "preferences", "preferences": sections, "count": sections.len(), "today": today}),
    )
}

fn card(scr: &Scr, private: bool) -> Value {
    let withheld = private || scr.front.private();
    let entries = scr.entries();
    json!({
        "id": scr.front.id, "path": scr.path, "title": scr.front.title,
        "title_by": scr.front.title_by, "scope": scr.front.scope_token(),
        "project": scr.front.project, "session": scr.front.session,
        "started": scr.front.started, "updated": scr.front.updated,
        "previous": scr.front.previous, "scribed_through": scr.front.scribed_through,
        "summary": if withheld { Value::Null } else { json!(scr.sections.summary) },
        "goals": if withheld { json!([]) } else { json!(scr.sections.goals) },
        "outcome": if withheld { json!([]) } else { json!(scr.sections.outcome) },
        "next": if withheld { json!([]) } else { json!(scr.sections.next) },
        "tag_entries": entries.len(),
        "tags": if withheld { json!([]) } else {
            json!(entries.iter().map(|e| json!({"id": e.id(), "citation": format!("{}#{}", scr.front.id, e.id()),
                "group": e.group, "msg": e.msg, "time": e.time, "by": e.by, "scope": e.scope,
                "quote": e.quote, "fields": e.fields.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<String, Value>>()}))
                .collect::<Vec<_>>())
        },
        "sensitivity": scr.front.sensitivity,
        "content_withheld": withheld,
    })
}

fn scr(call: &Call<'_>, input: &Map<String, Value>, private: bool) -> Result<Value> {
    let scope = scope_filter(input)?;
    let files = v2::v2_files(&call.ports.library.scribe_files()?)?;
    if let Some(id) = optional_text(input, "id")? {
        let one = files
            .iter()
            .find(|f| f.front.id == id)
            .ok_or_else(|| Error::Library {
                code: "scribe_scr_not_found",
                message: "no v2 SCR has this id".into(),
            })?;
        let previous = one
            .front
            .previous
            .as_deref()
            .and_then(|p| files.iter().find(|f| f.front.id == p));
        return Ok(json!({"mode": "scr", "scr": card(one, private),
            "previous": previous.map(|p| card(p, private))}));
    }
    let cards: Vec<Value> = v2::latest_per_scope(&files)
        .into_iter()
        .filter(|f| scope.as_deref().is_none_or(|s| f.front.scope_token() == s))
        .map(|f| card(f, private))
        .collect();
    Ok(json!({"mode": "scr", "cards": cards, "count": cards.len(), "files_searched": files.len()}))
}
