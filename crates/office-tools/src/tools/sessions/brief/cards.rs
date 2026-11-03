//! The `session-brief` `view: list` cards over legacy v2 session records
//! (SCR files already in the Library's `sources/conversation/`). Read-only.

use office_core::domain::source::conversation::scribe::v2::{self, Scr};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

/// One SCR as a feed card: its front matter, rewritten sections, Tags
/// entries and the per-tag entry counts (`tag_counts`, by Tags subsection).
/// A private SCR (or a private conversation setting) withholds its sections,
/// entries and counts; only references remain. Shared by `scribe-query`
/// mode `scr` and `session-brief` `view: list`.
pub(crate) fn card(scr: &Scr, private: bool) -> Value {
    let withheld = private || scr.front.private();
    let entries = scr.entries();
    let mut tag_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    if !withheld {
        for entry in &entries {
            *tag_counts.entry(entry.group.clone()).or_default() += 1;
        }
    }
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
        "tag_counts": tag_counts,
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

/// The cards answer over already-read v2 SCR files: with `id`, that one SCR
/// and the SCR its `previous:` names (the only way to reach an older SCR);
/// without one, exactly the latest SCR per project plus the latest global
/// SCR. Shared by `scribe-query` mode `scr` (which adds `mode` and the scope
/// filter) and `session-brief` `view: list`.
pub(crate) fn scr_cards(
    files: &[Scr],
    private: bool,
    id: Option<&str>,
    scope: Option<&str>,
) -> Result<Value> {
    if let Some(id) = id {
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
        return Ok(json!({"scr": card(one, private),
            "previous": previous.map(|p| card(p, private))}));
    }
    let cards: Vec<Value> = v2::latest_per_scope(files)
        .into_iter()
        .filter(|f| scope.is_none_or(|s| f.front.scope_token() == s))
        .map(|f| card(f, private))
        .collect();
    Ok(json!({"cards": cards, "count": cards.len(), "files_searched": files.len()}))
}
