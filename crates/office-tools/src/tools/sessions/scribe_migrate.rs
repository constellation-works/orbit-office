//! `office.scribe-migrate`: the one-time scribe v1 backfill
//! (specs/scribe.md, "Migration").
//!
//! Reads the v1 SCR lines and the conversation logs and files proposals,
//! never writing a store directly:
//!
//! - current v1 `#preference` lines and conversation-log `direction` lines
//!   become one preference proposal per scope and matter (`knowledge_add`
//!   for the scope's first page, else `knowledge_update` pinned to its
//!   revision);
//! - v1 `#decision` / `#agent-decision` lines and conversation-log
//!   `decision` lines become one `ledger_append` proposal per scope;
//! - a `dom-scribe-rules` page that still has the v1 Tags section or a
//!   `scribe:` mapping gets one `knowledge_update` removing them.
//!
//! Open v1 `#pending` lines are reported, not filed. Every proposal carries
//! the `scribe-migration` tag and a per-key tag, so a re-run files nothing
//! for a key already filed in any status. Private SCR files, private
//! conversations and a private conversation setting are skipped: the
//! migration never copies private text into a proposal.

use std::collections::BTreeMap;
use std::path::Path;

use office_core::domain::knowledge::preferences::{self, Preference};
use office_core::domain::knowledge::{DocumentRecord, Page, SCRIBE_RULES_PAGE};
use office_core::domain::metrics::metadata;
use office_core::domain::proposal::{self, Create, Payload};
use office_core::domain::source::conversation::corrections::effective_entries;
use office_core::domain::source::conversation::scribe;
use office_core::domain::source::decisions::Scope;
use office_core::domain::source::{sha256_hex, source_body};
use office_core::ports::Sensitivity;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use super::scribe_query::{Entry, entries_of, valid_date};
use crate::registry::{Call, Tool};
use crate::tools::insights::metrics::markdown;
use crate::tools::proposals::accept::verify_entries;
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
/// The global preference page every backfilled preference goes to.
const PREFERENCES_PAGE: &str = "dom-preferences-global";
/// Words of a direction kept in its derived matter slug.
const MATTER_WORDS: usize = 6;

/// One preference candidate: the latest current line for its matter.
struct PreferenceLine {
    statement: String,
    cite: String,
    stated_at: String,
    review_by: Option<String>,
}

/// One filing the migration plans.
struct Planned {
    key: String,
    kind: &'static str,
    scope: String,
    matter: Option<String>,
    entries: Option<usize>,
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

    let mut skipped_private = 0usize;
    let mut skipped_invalid = 0usize;
    let mut preferences: BTreeMap<String, PreferenceLine> = BTreeMap::new();
    let mut decisions: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
    let mut pending = Vec::new();

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
    v1_lines(
        &entries,
        &offsets,
        conversation_private,
        &mut skipped_private,
        &mut preferences,
        &mut decisions,
        &mut pending,
    );
    conversation_lines(
        call,
        conversation_private,
        &mut skipped_private,
        &mut preferences,
        &mut decisions,
    )?;

    let mut planned = Vec::new();
    preference_plans(call, &preferences, &mut planned)?;
    for (scope, mut lines) in decisions {
        // One entry per quote: v1 often recorded a decision both as an SCR
        // line and in the conversation log. Earliest first.
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let mut seen: Vec<String> = Vec::new();
        let mut entries = Vec::new();
        for (_, entry) in lines {
            // An entry the ledger would refuse is reported, never filed: it
            // would make the whole proposal unacceptable.
            let checked = office_core::domain::source::decisions::ledger_append_entries(
                &scope,
                std::slice::from_ref(&entry),
            )
            .and_then(|decisions| verify_entries(call, &decisions));
            if checked.is_err() {
                skipped_invalid += 1;
                continue;
            }
            let quote = entry
                .get("quote")
                .and_then(Value::as_str)
                .map(normalize)
                .unwrap_or_default();
            if seen.contains(&quote) {
                continue;
            }
            seen.push(quote);
            entries.push(entry);
        }
        if entries.len() > office_core::domain::source::decisions::LEDGER_APPEND_MAX {
            return Err(invalid(&format!(
                "found more than {} decisions for scope {scope}; one ledger_append proposal cannot carry them",
                office_core::domain::source::decisions::LEDGER_APPEND_MAX
            )));
        }
        if entries.is_empty() {
            continue;
        }
        let count = entries.len();
        let evidence = entries
            .iter()
            .filter_map(|entry| entry.get("source").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        planned.push(Planned {
            key: format!("ledger:{scope}"),
            kind: "ledger_append",
            scope: scope.clone(),
            matter: None,
            entries: Some(count),
            create: Create {
                title: format!("Backfill {count} scribe v1 decisions into the {scope} ledger"),
                description: format!(
                    "Scribe v2 migration: {count} decisions recorded by scribe v1 (SCR #decision and #agent-decision lines, conversation-log decision lines) for scope {scope}. Accepting appends every entry to the {scope} decision ledger in one commit, each with its v1 citation as source, or appends none if any entry fails the ledger checks."
                ),
                evidence,
                payload: Payload::LedgerAppend { scope, entries },
                tags: Vec::new(),
            },
        });
    }
    if let Some(plan) = rules_plan(call)? {
        planned.push(plan);
    }

    let existing = if dry_run && planned.is_empty() {
        BTreeMap::new()
    } else {
        filed_keys(call)?
    };
    let mut proposals = Vec::new();
    for mut plan in planned {
        let key_tag = format!("{TAG}:{}", &sha256_hex(plan.key.as_bytes())[..16]);
        let mut row = json!({
            "key": plan.key,
            "kind": plan.kind,
            "scope": plan.scope,
            "matter": plan.matter,
            "entries": plan.entries,
        });
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
        row["status"] = json!(status);
        row["id"] = json!(id);
        proposals.push(row);
    }
    Ok(json!({
        "dry_run": dry_run,
        "proposals": proposals,
        "pending": pending,
        "skipped_private": skipped_private,
        "skipped_invalid": skipped_invalid,
    }))
}

/// Sort the v1 SCR lines into the preference, decision and pending sets.
fn v1_lines(
    entries: &[Entry],
    offsets: &BTreeMap<String, usize>,
    conversation_private: bool,
    skipped_private: &mut usize,
    preferences: &mut BTreeMap<String, PreferenceLine>,
    decisions: &mut BTreeMap<String, Vec<(String, Value)>>,
    pending: &mut Vec<Value>,
) {
    let mut superseded: Vec<&str> = Vec::new();
    let mut resolved: Vec<&str> = Vec::new();
    for entry in entries {
        match (entry.tag.as_str(), &entry.supersedes, &entry.resolves) {
            ("preference", Some(target), _) => superseded.push(target),
            ("resolved", _, Some(target)) => resolved.push(target),
            _ => {}
        }
    }
    let names = |entry: &Entry, targets: &[&str]| {
        entry
            .keys()
            .iter()
            .any(|key| targets.contains(&key.as_str()))
    };
    // Entries are in time order, so a later current line on a matter wins.
    for entry in entries {
        let tag = entry.tag.as_str();
        if !matches!(
            tag,
            "preference" | "decision" | "agent-decision" | "pending"
        ) {
            continue;
        }
        if entry.private || conversation_private {
            *skipped_private += 1;
            continue;
        }
        let offset = offsets.get(&entry.path).copied().unwrap_or_default();
        let cite = format!("{}#L{}", entry.scr_id, entry.line.saturating_sub(offset));
        let quote = entry.quote.trim();
        if quote.is_empty() {
            continue;
        }
        match tag {
            "preference" if !names(entry, &superseded) => {
                let matter = entry
                    .matter
                    .as_deref()
                    .filter(|m| is_slug(m))
                    .map_or_else(|| slugify(quote), str::to_owned);
                preferences.insert(
                    matter,
                    PreferenceLine {
                        statement: one_line(quote),
                        cite,
                        stated_at: date_of(&entry.time),
                        review_by: entry.review_by.clone().filter(|d| valid_date(d)),
                    },
                );
            }
            "decision" | "agent-decision" => {
                let Some(decided_at) = rfc3339(&entry.time) else {
                    continue;
                };
                let scope = scope_of(entry.project.as_deref());
                let agent =
                    tag == "agent-decision" || entry.by.as_deref().is_some_and(|by| by != "user");
                let question = format!(
                    "What was decided in {}?",
                    entry
                        .title
                        .as_deref()
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or(&entry.scr_id)
                );
                let because = entry
                    .why_quote
                    .as_deref()
                    .filter(|why| !why.trim().is_empty())
                    .map(|why| json!({"text": why, "ref": cite}));
                decisions.entry(scope.clone()).or_default().push((
                    decided_at.clone(),
                    ledger_entry(&scope, &question, quote, &cite, because, agent, &decided_at),
                ));
            }
            "pending" if !names(entry, &resolved) => pending.push(json!({
                "cite": cite,
                "quote": quote,
                "time": entry.time,
                "session_id": entry.session_id,
                "due": entry.due,
            })),
            _ => {}
        }
    }
}

/// Add the conversation-log `direction` and `decision` lines.
fn conversation_lines(
    call: &Call<'_>,
    conversation_private: bool,
    skipped_private: &mut usize,
    preferences: &mut BTreeMap<String, PreferenceLine>,
    decisions: &mut BTreeMap<String, Vec<(String, Value)>>,
) -> Result<()> {
    let mut directions: Vec<(String, String, PreferenceLine)> = Vec::new();
    for file in call.ports.library.source_documents()? {
        let text = markdown(&file)?;
        let fields = metadata(text)?;
        if fields.get("kind").and_then(Value::as_str) != Some("conversation") {
            continue;
        }
        let record = DocumentRecord::from_markdown(Path::new(&file.path), text);
        let entries = effective_entries(&source_body(text))?;
        let relevant = entries
            .iter()
            .filter(|e| e.kind == "direction" || e.kind == "decision")
            .count();
        if conversation_private || record.sensitivity != "normal" {
            *skipped_private += relevant;
            continue;
        }
        let project = fields
            .get("project")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                record
                    .tags
                    .iter()
                    .find_map(|tag| tag.strip_prefix("project:").map(str::to_owned))
            });
        let scope = scope_of(project.as_deref());
        for entry in entries {
            let quote = entry.text.trim();
            if quote.is_empty() {
                continue;
            }
            let cite = format!("{}#L{}", record.id, entry.line);
            match entry.kind.as_str() {
                "direction" => directions.push((
                    entry.time.clone(),
                    slugify(quote),
                    PreferenceLine {
                        statement: one_line(quote),
                        cite,
                        stated_at: date_of(&entry.time),
                        review_by: None,
                    },
                )),
                "decision" => {
                    let Some(decided_at) = rfc3339(&entry.time) else {
                        continue;
                    };
                    let question = format!("What was decided in {}?", record.title);
                    let agent = entry.by.as_deref() != Some("user");
                    decisions.entry(scope.clone()).or_default().push((
                        decided_at.clone(),
                        ledger_entry(&scope, &question, quote, &cite, None, agent, &decided_at),
                    ));
                }
                _ => {}
            }
        }
    }
    // A direction is a current preference unless a v1 preference or a later
    // direction states the same matter.
    directions.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, matter, line) in directions {
        if let Some(existing) = preferences.get(&matter)
            && existing.cite.starts_with("SCR-")
        {
            continue;
        }
        preferences.insert(matter, line);
    }
    Ok(())
}

/// One proposal per matter, each the current page with that matter set.
fn preference_plans(
    call: &Call<'_>,
    preferences: &BTreeMap<String, PreferenceLine>,
    planned: &mut Vec<Planned>,
) -> Result<()> {
    if preferences.is_empty() {
        return Ok(());
    }
    let current = match call.ports.library.get(PREFERENCES_PAGE)? {
        Some(record) => {
            let text = call.ports.library.read_text(Path::new(&record.path))?;
            Some((Page::from_markdown(&text)?, sha256_hex(text.as_bytes())))
        }
        None => None,
    };
    let sections = match &current {
        Some((page, _)) => preferences::parse(&page.id, &page.body)?,
        None => Vec::new(),
    };
    let title = current.as_ref().map_or_else(
        || "Global preferences".to_owned(),
        |(page, _)| page.title.clone(),
    );
    for (matter, line) in preferences {
        if line.stated_at.is_empty() {
            continue;
        }
        if sections
            .iter()
            .any(|s| &s.matter == matter && s.statement.trim() == line.statement.trim())
        {
            continue;
        }
        let review_by = line
            .review_by
            .clone()
            .filter(|review| review.as_str() >= line.stated_at.as_str());
        let preference = Preference::new(
            matter.clone(),
            line.statement.clone(),
            vec![line.cite.clone()],
            line.stated_at.clone(),
            review_by,
        )?;
        let mut body = String::new();
        let mut replaced = false;
        for section in &sections {
            if &section.matter == matter {
                body.push_str(&preference.markdown());
                replaced = true;
            } else {
                body.push_str(&section.markdown());
            }
        }
        if !replaced {
            body.push_str(&preference.markdown());
        }
        let markdown = format!(
            "---\nid: {PREFERENCES_PAGE}\ntitle: {}\ntemplate: preferences\n---\n{body}",
            json!(title)
        );
        let payload = match &current {
            Some((_, sha)) => Payload::KnowledgeUpdate {
                conflicts: Vec::new(),
                shareable: None,
                markdown,
                base_sha256: sha.clone(),
            },
            None => Payload::KnowledgeAdd {
                conflicts: Vec::new(),
                shareable: None,
                page: None,
                markdown: Some(markdown),
            },
        };
        planned.push(Planned {
            key: format!("pref:global:{matter}"),
            kind: payload.kind().as_str(),
            scope: "global".to_owned(),
            matter: Some(matter.clone()),
            entries: None,
            create: Create {
                title: format!("Preference: {matter}"),
                description: format!(
                    "Scribe v2 migration: the current scribe v1 preference ({}): {}\n\nAccepting sets the {matter} section of {PREFERENCES_PAGE}.",
                    line.cite, line.statement
                ),
                evidence: vec![line.cite.clone()],
                payload,
                tags: Vec::new(),
            },
        });
    }
    Ok(())
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
        scope: "global".to_owned(),
        matter: None,
        entries: None,
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
pub(crate) fn without_v1_rules(text: &str) -> Option<String> {
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

fn ledger_entry(
    scope: &str,
    question: &str,
    quote: &str,
    cite: &str,
    because: Option<Value>,
    agent: bool,
    decided_at: &str,
) -> Value {
    json!({
        "question": one_line(question),
        "question_by": "scribe",
        "options": "unrecoverable",
        "chosen": quote,
        "quote": quote,
        "ref": cite,
        "because": because,
        "scope": scope,
        "decided_by": if agent { "agent" } else { "human" },
        "decided_at": decided_at,
        "source": cite,
    })
}

/// `project:<slug>` when the project name is a valid scope, else `global`.
fn scope_of(project: Option<&str>) -> String {
    project
        .map(|name| format!("project:{name}"))
        .and_then(|token| Scope::parse(&token).ok())
        .map_or_else(|| "global".to_owned(), |scope| scope.token())
}

/// An RFC 3339 time with seconds and an offset. v1 SCR times omit the
/// seconds (`2026-10-30T10:00-07:00`); they are read as `:00`.
fn rfc3339(time: &str) -> Option<String> {
    if time.len() < 17 || !valid_date(time.get(..10)?) || time.get(10..11)? != "T" {
        return None;
    }
    let time = if time.get(16..17)? == ":" {
        time.to_owned()
    } else {
        format!("{}:00{}", time.get(..16)?, time.get(16..)?)
    };
    let offset = time.get(19..)?;
    let offset = offset.strip_prefix('.').map_or(offset, |rest| {
        rest.trim_start_matches(|c: char| c.is_ascii_digit())
    });
    let offset_ok = offset == "Z"
        || (offset.len() == 6
            && (offset.starts_with('+') || offset.starts_with('-'))
            && offset.get(3..4) == Some(":"));
    offset_ok.then_some(time)
}

fn date_of(time: &str) -> String {
    time.get(..10)
        .filter(|d| valid_date(d))
        .map_or_else(String::new, str::to_owned)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize(text: &str) -> String {
    one_line(text).to_lowercase()
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A matter slug from the first words of a statement.
fn slugify(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(MATTER_WORDS)
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        "preference".to_owned()
    } else {
        words.join("-")
    }
}
