//! Orbit decision ingestion (specs/decision-precedents.md, "Ingesting Orbit
//! decisions" and "Feedback"): answered Orbit human-decision requests become
//! decision ledger entries, one per request answer.
//!
//! Pure: the tool reads the requests and the ledgers through ports and
//! hands them here; this module selects, maps, links and plans the ledger
//! files one commit writes. Nothing here reads Orbit or the Library.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};

use super::conversation::scribe::v2::PlannedFile;
use super::decisions::{
    Cited, Context, DecidedBy, Decision, Ledger, Options, OrbitInfo, OrbitModel, Scope,
    append_line, chosen_matches,
};
use crate::domain::metrics::timestamp;
use crate::{Error, Result};

/// The Orbit actor a curated-rule auto-answer is recorded under; never a
/// human decision, so never ingested.
pub const PRECEDENT_ACTOR: &str = "precedent";
/// The Orbit actor a decision model's answer is recorded under.
pub const MODEL_ACTOR: &str = "model";
/// The gate Orbit raises to confirm or veto a model answer.
pub const MODEL_ANSWER_REVIEW: &str = "model_answer_review";
/// The `source`/`ref` prefix of an ingested entry.
pub const SOURCE_PREFIX: &str = "orbit-decision:";

/// One request as `orbit.decision.list` returned it, beside the followed
/// workspace selector it was listed under (`None`: the Office workspace).
#[derive(Clone, Debug)]
pub struct Listed {
    /// `brief.task_workspaces` selector, or `None` for the Office workspace.
    pub selector: Option<String>,
    /// The request record, verbatim.
    pub request: Value,
}

/// What a run plans: the ledger files to write and the report.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// Ledger files, compare-and-set against the text read.
    pub files: Vec<PlannedFile>,
    /// `{request_id, entry, ledger, gate, decided_by, same_as, confirms, reverses}`.
    pub appended: Vec<Value>,
    /// `{request_id, reason}` (and `code`, `message` for `invalid`).
    pub skipped: Vec<Value>,
    /// `{request_id, entry, outcome, same_as | candidates}`.
    pub dedupe: Vec<Value>,
}

#[derive(Deserialize)]
struct Answer {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    consequence: Option<String>,
}

#[derive(Deserialize)]
struct Resolution {
    answer_id: String,
    answered_by: String,
    #[serde(default)]
    provenance: Option<String>,
    #[serde(default)]
    note: Option<String>,
    /// The applied-at time. Orbit names it `answered_at`; `applied_at` is
    /// read too, as the design names it.
    #[serde(default)]
    answered_at: Option<String>,
    #[serde(default)]
    applied_at: Option<String>,
    #[serde(default)]
    model: Option<OrbitModel>,
}

#[derive(Deserialize)]
struct Request {
    id: String,
    task_id: String,
    gate: String,
    #[serde(default)]
    answers: Vec<Answer>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    resolution: Option<Resolution>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    risk_tier: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    raised_at: Option<String>,
    #[serde(default)]
    binding: Option<Value>,
    #[serde(default)]
    review_of: Option<String>,
}

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line(text: Option<&str>) -> Option<String> {
    text.map(words).filter(|text| !text.is_empty())
}

/// The request id a `model_answer_review` request reviews: its binding's
/// `request_id` (Orbit human-decision-channel §11.3, "bound to the
/// original request id"), else a top-level `review_of`.
fn reviewed_request(request: &Request) -> Option<String> {
    request
        .binding
        .as_ref()
        .and_then(|binding| binding.get("request_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| request.review_of.clone())
        .filter(|id| !id.trim().is_empty())
}

/// The fixed wording of a gate's question when the request carries none.
fn gate_question(request: &Request, reviewed: Option<&str>) -> String {
    let task = &request.task_id;
    match request.gate.as_str() {
        "block_triage" => format!("How should blocked task {task} proceed?"),
        "pilot_admission" => format!("Should task {task} be admitted past its pilot hold?"),
        "proposed_approval" => format!("Should proposed task {task} be approved?"),
        "review_approval" => format!("Should the candidate under review for task {task} land?"),
        "agent_request" => format!("What should the agent on task {task} do?"),
        MODEL_ANSWER_REVIEW => format!(
            "Does the decision model's answer to {} on task {task} stand?",
            reviewed.unwrap_or("its request")
        ),
        other => format!("Decision on task {task} ({other})"),
    }
}

/// The workspace id a request belongs to: its category's leading
/// `<workspace_id>/` (Orbit §11.1), else the followed selector's last
/// `ws_` component. `None` when neither names one.
#[must_use]
pub fn workspace_of(category: Option<&str>, selector: Option<&str>) -> Option<String> {
    let from_category = category
        .and_then(|category| category.split(['/', ':']).next())
        .filter(|head| head.starts_with("ws_"));
    let from_selector = selector
        .and_then(|selector| selector.rsplit('/').next())
        .filter(|tail| tail.starts_with("ws_"));
    from_category.or(from_selector).map(str::to_owned)
}

/// The ledger scope of a workspace: `project:<name>` for `ws_<name>` when
/// `<name>` is a Library project slug, else `global`.
#[must_use]
pub fn scope_of(workspace: Option<&str>) -> Scope {
    workspace
        .and_then(|workspace| workspace.strip_prefix("ws_"))
        .and_then(|name| Scope::parse(&format!("project:{name}")).ok())
        .unwrap_or(Scope::Global)
}

/// Every scribe or captured entry (no `orbit`) that records the same
/// decision as the ingested `entry`: the same task, `decided_at` within
/// 30 minutes, and a `chosen` that matches the request's chosen answer by
/// folding or containment, that answer being the only match or the exact
/// one.
fn scribe_matches(entry: &Decision, answers: &[Cited], ledgers: &[Working]) -> Vec<String> {
    let (Some(task), Some(chosen)) = (entry.context.task.as_deref(), entry.chosen.as_deref())
    else {
        return Vec::new();
    };
    let Some(at) = timestamp(&entry.decided_at) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for working in ledgers {
        for existing in &working.ledger.entries {
            let candidate = &existing.decision;
            if candidate.orbit.is_some()
                || candidate.private
                || candidate.context.task.as_deref() != Some(task)
            {
                continue;
            }
            let Some(candidate_at) = timestamp(&candidate.decided_at) else {
                continue;
            };
            if (at - candidate_at).abs() > 30 * 60 {
                continue;
            }
            let Some(said) = candidate.chosen.as_deref() else {
                continue;
            };
            let matching: Vec<&Cited> = answers
                .iter()
                .filter(|answer| chosen_matches(said, &answer.text))
                .collect();
            let picks_chosen = match matching.as_slice() {
                [] => false,
                [only] => only.text == chosen,
                _ => {
                    words(&said.to_lowercase()) == words(&chosen.to_lowercase())
                        && matching.iter().any(|answer| answer.text == chosen)
                }
            };
            if picks_chosen {
                found.push(candidate.id.clone());
            }
        }
    }
    found
}

struct Working {
    ledger: Ledger,
    text: String,
    expected: Option<String>,
    changed: bool,
}

/// An ingested entry already in a ledger: its id, request and answer time.
struct Ingested {
    entry: String,
    request: String,
    decided_at: Option<i64>,
    decided_by: DecidedBy,
}

fn ingested(ledgers: &[Working]) -> Vec<Ingested> {
    ledgers
        .iter()
        .flat_map(|working| &working.ledger.entries)
        .filter_map(|entry| {
            let decision = &entry.decision;
            let request = decision.orbit.as_ref()?.request_id.clone()?;
            Some(Ingested {
                entry: decision.id.clone(),
                request,
                decided_at: timestamp(&decision.decided_at),
                decided_by: decision.decided_by,
            })
        })
        .collect()
}

fn skip(skipped: &mut Vec<Value>, request: &str, reason: &str) {
    skipped.push(json!({"request_id": request, "reason": reason}));
}

/// Plan one ingestion run.
///
/// `ledgers` are the ledger files as read (path, Markdown); `listed` every
/// request the followed workspaces returned; `since` drops answers applied
/// before it (and closed unanswered requests raised before it); `now`
/// becomes a new ledger's `created_at`. `task_type` returns an Orbit
/// task's type, or `None` when unknown.
pub fn plan(
    ledgers: &[(String, String)],
    listed: &[Listed],
    since: Option<&str>,
    now: &str,
    mut task_type: impl FnMut(&str, Option<&str>) -> Option<String>,
) -> Result<Plan> {
    let since = match since {
        None => None,
        Some(text) => Some(timestamp(text).ok_or_else(|| {
            Error::InvalidArgument("decision-ingest since must be an RFC 3339 time".into())
        })?),
    };
    let mut working = Vec::new();
    for (path, text) in ledgers {
        working.push(Working {
            ledger: Ledger::parse(path, text)?,
            text: text.clone(),
            expected: Some(text.clone()),
            changed: false,
        });
    }
    let mut out = Plan::default();
    let mut seen = BTreeSet::new();
    let mut ready: Vec<(i64, Request, Option<String>)> = Vec::new();
    for item in listed {
        let request: Request = match serde_json::from_value(item.request.clone()) {
            Ok(request) => request,
            Err(_) => {
                let id = item
                    .request
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                out.skipped
                    .push(json!({"request_id": id, "reason": "unreadable"}));
                continue;
            }
        };
        if !seen.insert(request.id.clone()) {
            continue;
        }
        let raised = request.raised_at.as_deref().and_then(timestamp);
        let before_since =
            |at: Option<i64>| matches!((since, at), (Some(since), Some(at)) if at < since);
        match request.state.as_deref() {
            Some("answered") => {}
            Some(state @ ("superseded" | "expired")) => {
                if !before_since(raised) {
                    skip(&mut out.skipped, &request.id, state);
                }
                continue;
            }
            // Open (or an unknown future state): nothing answered yet.
            _ => continue,
        }
        let Some(resolution) = &request.resolution else {
            skip(&mut out.skipped, &request.id, "resolution_missing");
            continue;
        };
        let at = resolution
            .answered_at
            .as_deref()
            .or(resolution.applied_at.as_deref())
            .and_then(timestamp);
        let Some(at) = at else {
            skip(&mut out.skipped, &request.id, "resolution_missing");
            continue;
        };
        if before_since(Some(at)) {
            continue;
        }
        if resolution.answered_by == PRECEDENT_ACTOR {
            skip(&mut out.skipped, &request.id, "precedent_actor");
            continue;
        }
        if ingested(&working)
            .iter()
            .any(|entry| entry.request == request.id && entry.decided_at == Some(at))
        {
            skip(&mut out.skipped, &request.id, "already_ingested");
            continue;
        }
        ready.push((at, request, item.selector.clone()));
    }
    // Oldest answer first, so a model entry precedes its review and a
    // re-answer follows the answer it reverses.
    ready.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let mut task_types: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (_, request, selector) in ready {
        ingest_one(
            &request,
            selector.as_deref(),
            now,
            &mut working,
            &mut task_types,
            &mut task_type,
            &mut out,
        );
    }
    for working in working.into_iter().filter(|working| working.changed) {
        out.files.push(PlannedFile {
            path: working.ledger.path.clone(),
            text: working.text,
            expected: working.expected,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_lines)]
fn ingest_one(
    request: &Request,
    selector: Option<&str>,
    now: &str,
    working: &mut Vec<Working>,
    task_types: &mut BTreeMap<String, Option<String>>,
    task_type: &mut impl FnMut(&str, Option<&str>) -> Option<String>,
    out: &mut Plan,
) {
    let Some(resolution) = &request.resolution else {
        return;
    };
    let decided_at = resolution
        .answered_at
        .clone()
        .or_else(|| resolution.applied_at.clone())
        .unwrap_or_default();
    let decided_by = if resolution.answered_by == MODEL_ACTOR {
        DecidedBy::Model
    } else {
        DecidedBy::Human
    };
    let Some(chosen) = request
        .answers
        .iter()
        .find(|answer| answer.id == resolution.answer_id)
    else {
        skip(&mut out.skipped, &request.id, "answer_unknown");
        return;
    };
    let chosen_label = line(chosen.label.as_deref()).unwrap_or_else(|| words(&chosen.id));
    let reference = format!("{SOURCE_PREFIX}{}", request.id);
    let answers: Vec<Cited> = request
        .answers
        .iter()
        .map(|answer| Cited {
            text: line(answer.label.as_deref()).unwrap_or_else(|| words(&answer.id)),
            reference: format!("{reference}#{}", answer.id),
            id: Some(words(&answer.id)),
            consequence: line(answer.consequence.as_deref()),
        })
        .collect();
    let previous = ingested(working);
    // Feedback links (decision-precedents.md, Feedback).
    let mut reverses = None;
    let mut confirms = None;
    let reviewed = reviewed_request(request);
    if request.gate == MODEL_ANSWER_REVIEW {
        let Some(target) = reviewed.as_deref() else {
            skip(&mut out.skipped, &request.id, "review_target_missing");
            return;
        };
        let Some(model_entry) = previous
            .iter()
            .filter(|entry| entry.request == target && entry.decided_by == DecidedBy::Model)
            .max_by_key(|entry| entry.decided_at)
        else {
            skip(&mut out.skipped, &request.id, "review_target_missing");
            return;
        };
        if decided_by == DecidedBy::Human {
            match resolution.answer_id.as_str() {
                "confirm" => confirms = Some(model_entry.entry.clone()),
                "veto" => reverses = Some(model_entry.entry.clone()),
                _ => {}
            }
        }
    } else if let Some(prior) = previous
        .iter()
        .filter(|entry| entry.request == request.id)
        .max_by_key(|entry| entry.decided_at)
    {
        // The request was answered again after a veto re-opened it: the
        // new answer overrides the one recorded before.
        reverses = Some(prior.entry.clone());
    }
    let workspace = workspace_of(request.category.as_deref(), selector);
    let scope = scope_of(workspace.as_deref());
    let task_kind = task_types
        .entry(request.task_id.clone())
        .or_insert_with(|| task_type(&request.task_id, selector))
        .clone();
    let note = line(resolution.note.as_deref());
    // `chosen` must be a verbatim span of `quote`: the note is the quote
    // when it contains the label, else the label leads the note.
    let quote = match note {
        Some(note) if note.contains(&chosen_label) => note,
        Some(note) => format!("{chosen_label}\n{note}"),
        None => chosen_label.clone(),
    };
    let question = line(request.question.as_deref())
        .or_else(|| line(request.summary.as_deref()))
        .unwrap_or_else(|| gate_question(request, reviewed.as_deref()));
    let index = match working
        .iter()
        .position(|working| working.ledger.scope == scope)
    {
        Some(index) => index,
        None => {
            let ledger = Ledger::new(scope.clone(), now);
            let Ok(text) = ledger.render() else {
                return;
            };
            working.push(Working {
                ledger,
                text,
                expected: None,
                changed: false,
            });
            working.len() - 1
        }
    };
    let mut decision = Decision {
        id: working[index].ledger.next_id(),
        question: Some(question),
        question_ref: Some(reference.clone()),
        question_by: None,
        options: Some(Options::Listed(answers.clone())),
        chosen: Some(chosen_label),
        quote: Some(quote),
        reference: reference.clone(),
        because: None,
        scope: scope.token(),
        context: Context {
            task_type: line(task_kind.as_deref()),
            workspace,
            task: Some(request.task_id.clone()),
            risk_tier: line(request.risk_tier.as_deref()),
        },
        decided_by,
        decided_at,
        reverses: reverses.clone(),
        orbit: Some(OrbitInfo {
            request_id: Some(request.id.clone()),
            gate: line(Some(&request.gate)),
            category: line(request.category.as_deref()),
            topic: line(request.topic.as_deref()),
            answered_by: line(Some(&resolution.answered_by)),
            channel: line(resolution.provenance.as_deref()),
            model: if decided_by == DecidedBy::Model {
                resolution.model.clone()
            } else {
                None
            },
        }),
        same_as: None,
        confirms: confirms.clone(),
        source: reference,
        private: false,
    };
    let matches = scribe_matches(&decision, &answers, working);
    match matches.as_slice() {
        [] => {}
        [only] => {
            decision.same_as = Some(only.clone());
            out.dedupe.push(json!({
                "request_id": request.id, "entry": decision.id,
                "outcome": "same_as", "same_as": only,
            }));
        }
        several => out.dedupe.push(json!({
            "request_id": request.id, "entry": decision.id,
            "outcome": "dedupe_ambiguous", "candidates": several,
        })),
    }
    let refused = |out: &mut Plan, error: &Error| {
        let (code, message) = if let Error::Library { code, message } = error {
            ((*code).to_owned(), message.clone())
        } else {
            ("invalid_decision".to_owned(), error.to_string())
        };
        out.skipped.push(json!({
            "request_id": request.id, "reason": "invalid", "code": code, "message": message,
        }));
    };
    if let Err(error) = decision.validate() {
        out.dedupe
            .retain(|item| item["entry"] != json!(decision.id));
        refused(out, &error);
        return;
    }
    let line_text = match decision.line() {
        Ok(text) => text,
        Err(error) => {
            refused(out, &error);
            return;
        }
    };
    let target = &mut working[index];
    let text = append_line(&target.text, &line_text);
    match Ledger::parse(&target.ledger.path, &text) {
        Ok(parsed) => {
            target.ledger = parsed;
            target.text = text;
            target.changed = true;
        }
        Err(error) => {
            out.dedupe
                .retain(|item| item["entry"] != json!(decision.id));
            refused(out, &error);
            return;
        }
    }
    out.appended.push(json!({
        "request_id": request.id,
        "entry": decision.id,
        "ledger": scope.ledger_id(),
        "gate": request.gate,
        "decided_by": decided_by,
        "same_as": decision.same_as,
        "confirms": confirms,
        "reverses": reverses,
    }));
}
