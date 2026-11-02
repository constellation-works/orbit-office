//! One scribe v2 pass, planned without a model or I/O (specs/scribe.md,
//! "Pipeline" step 4). The caller reads the stores, calls [`plan_pass`],
//! and writes the SCR and every planned ledger or page file in one commit.
//!
//! Every finding is checked against the model-free marker list: its
//! marker must be one `office.scribe-pending` returned for this window,
//! its verbatim fields exact text of the cited messages, and its author
//! claims the marker's. A refused finding is reported, never written.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::{Message, Parsed, line_time, local, message_index};
use super::markers::{self, Author, Marker};
use super::scr::{self, Front, Scr, Sections, TagEntry, TagGroup};
use crate::domain::knowledge::preferences::{self, Conflict, Preference};
use crate::domain::scribe_settings::{ScribeTag, TagMode, TagSink};
use crate::domain::source::decisions::{
    Cited, Context, DecidedBy, Decision, Ledger, Options, Scope, append_line, same_as_match,
};
use crate::ports::LibraryText;
use crate::{Error, Result};

/// Longest scribe title, in words.
pub const TITLE_WORDS: usize = 8;
/// Library directory of the preference pages.
pub const PREFERENCE_DIR: &str = "knowledge/domain";

/// One pass request, as `office.scribe-append` received it.
#[derive(Clone, Copy, Debug)]
pub struct PassRequest<'a> {
    /// Provider session id.
    pub session_id: &'a str,
    /// The last message this pass read.
    pub through: &'a str,
    /// A title: the scribe's proposal, or the user's.
    pub title: Option<&'a str>,
    /// `scribe` (default) or `user`.
    pub title_by: Option<&'a str>,
    /// `{summary, goals, outcome, next}`.
    pub sections: &'a Value,
    /// The findings list, one per marker.
    pub findings: Option<&'a Value>,
    /// Now, RFC 3339.
    pub now: &'a str,
    /// The host time zone.
    pub zone: Option<&'a str>,
}

/// What the pass reads, already loaded by the caller.
#[derive(Clone, Copy, Debug)]
pub struct PassStores<'a> {
    /// The parsed messages.
    pub messages: &'a [Message],
    /// The session facts.
    pub parsed: &'a Parsed,
    /// The tag definitions in force.
    pub tags: &'a [ScribeTag],
    /// Every v2 SCR.
    pub files: &'a [Scr],
    /// Every decision ledger, parsed, beside its file text.
    pub ledgers: &'a [(Ledger, String)],
    /// The Library's knowledge pages (the preference pages among them).
    pub pages: &'a [LibraryText],
}

/// One ledger or preference page the pass writes, compare-and-set:
/// `expected` is the text read before planning (`None`: the file must not
/// exist yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedFile {
    /// Library-relative path.
    pub path: String,
    /// Full new text.
    pub text: String,
    /// The text planned against.
    pub expected: Option<String>,
}

/// The planned pass.
#[derive(Clone, Debug)]
pub struct PassPlan {
    /// The SCR id.
    pub id: String,
    /// Library-relative SCR path.
    pub path: String,
    /// The new front matter.
    pub front: Front,
    /// The new SCR text; `None` when nothing is written.
    pub markdown: Option<String>,
    /// The SCR as read (`None`: this pass creates it).
    pub current: Option<String>,
    /// The scope token.
    pub scope: String,
    /// Ledger and preference page writes, landing in the same commit.
    pub files: Vec<PlannedFile>,
    /// Tags entries added: `{id, group, msg, line}`.
    pub added: Vec<Value>,
    /// Ledger entries appended: `{id, ledger, entry}`.
    pub ledger: Vec<Value>,
    /// Preference sections written in apply mode: `{entry, page, matter}`.
    pub applied: Vec<Value>,
    /// Preference findings the plugin files as proposals.
    pub needs_proposal: Vec<Value>,
    /// Findings withheld from shared stores (a private session).
    pub withheld: Vec<Value>,
    /// Agent and instruction markers reported back, not filed.
    pub reported: Vec<Value>,
    /// Refused findings: `{msg, line, tag, code, reason}`.
    pub refused: Vec<Value>,
    /// Findings already recorded: `{msg, line, entry}`.
    pub duplicates: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionsInput {
    summary: String,
    #[serde(default)]
    goals: Vec<String>,
    #[serde(default)]
    outcome: Vec<String>,
    #[serde(default)]
    next: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Span {
    text: String,
    msg: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionInput {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    question_ref: Option<String>,
    #[serde(default)]
    question_by: Option<String>,
    #[serde(default)]
    options: Option<Value>,
    #[serde(default)]
    chosen: Option<String>,
    #[serde(default)]
    because: Option<Span>,
    #[serde(default)]
    context: Option<Context>,
    #[serde(default)]
    decided_by: Option<DecidedBy>,
    #[serde(default)]
    reverses: Option<String>,
    #[serde(default)]
    same_as: Option<String>,
    #[serde(default)]
    orbit: Option<Value>,
    #[serde(default)]
    confirms: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    msg: String,
    line: usize,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    decision: Option<DecisionInput>,
    #[serde(default)]
    matter: Option<String>,
    #[serde(default)]
    statement: Option<String>,
    #[serde(default)]
    conflicts: Vec<Conflict>,
    #[serde(default)]
    proposal: Option<String>,
    #[serde(default)]
    review_by: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArgument(message.into())
}

fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

/// A refused finding: reported, never written.
struct Refused {
    code: &'static str,
    reason: String,
}

fn refused(code: &'static str, reason: impl Into<String>) -> Refused {
    Refused {
        code,
        reason: reason.into(),
    }
}

/// One token: no whitespace, nonempty, printable.
fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && value.chars().all(|c| c.is_ascii_graphic())
}

/// A complete Orbit task id such as `ORB-<n>`.
fn task_id(value: &str) -> bool {
    let Some((prefix, rest)) = value.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.bytes().all(|b| b.is_ascii_uppercase())
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && rest.bytes().any(|b| b.is_ascii_digit())
}

fn refs(items: &[String]) -> Vec<&str> {
    items.iter().map(String::as_str).collect()
}

fn rfc3339(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

fn title_words(text: &str) -> String {
    text.trim_start_matches("(scribe)")
        .split_whitespace()
        .take(TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ',', ';', ':', '!', '?'])
        .to_owned()
}

/// The preference page id of a scope: `dom-preferences-<slug>`.
#[must_use]
pub fn preference_page_id(scope: &Scope) -> String {
    format!("dom-preferences-{}", scope.slug())
}

/// `knowledge/domain/dom-preferences-<slug>.md`.
#[must_use]
pub fn preference_page_path(scope: &Scope) -> String {
    format!("{PREFERENCE_DIR}/{}.md", preference_page_id(scope))
}

fn new_page(scope: &Scope) -> String {
    let title = match scope {
        Scope::Global => "Global preferences".to_owned(),
        Scope::Project(name) => format!("{name} preferences"),
    };
    format!(
        "---\nid: {}\ntitle: {}\ntemplate: preferences\n---\n",
        preference_page_id(scope),
        serde_json::Value::String(title)
    )
}

/// The page text with `preference` set on its matter: a new matter is
/// appended, the same statement refreshes its evidence and dates. `None`
/// when the matter holds a different statement (a conflict).
fn with_preference(text: &str, id: &str, preference: &Preference) -> Result<Option<String>> {
    let body = crate::domain::source::source_body(text);
    let head = text.get(..text.len() - body.len()).unwrap_or_default();
    let mut records = if body.trim().is_empty() {
        Vec::new()
    } else {
        preferences::parse(id, &body)?
    };
    match records.iter_mut().find(|r| r.matter == preference.matter) {
        Some(current) if current.statement.trim() != preference.statement.trim() => {
            return Ok(None);
        }
        Some(current) => {
            for cite in &preference.cites {
                if !current.cites.contains(cite) {
                    current.cites.push(cite.clone());
                }
            }
            current.stated_at.clone_from(&preference.stated_at);
            current.review_by.clone_from(&preference.review_by);
        }
        None => records.push(preference.clone()),
    }
    let mut out = head.to_owned();
    for record in &records {
        out.push_str(&record.markdown());
    }
    Ok(Some(out))
}

/// One accepted finding, before rendering.
struct Accepted {
    group: String,
    number: usize,
    msg: String,
    line: usize,
    time: String,
    by: Author,
    scope: String,
    quote: String,
    fields: Vec<(String, String)>,
    /// The preference page an apply-mode finding writes.
    page: Option<String>,
    /// The proposal a fallback reports.
    proposal: Option<Value>,
}

/// The working copy of a ledger or page file.
struct Working {
    text: String,
    expected: Option<String>,
    changed: bool,
}

struct Pass<'a> {
    request: PassRequest<'a>,
    stores: PassStores<'a>,
    scr_id: String,
    private: bool,
    ledgers: BTreeMap<String, (Ledger, Working)>,
    pages: BTreeMap<String, Working>,
}

impl Pass<'_> {
    fn message(&self, id: &str, through: usize) -> Option<&Message> {
        message_index(id)
            .filter(|index| *index <= through)
            .and_then(|index| self.stores.messages.get(index))
    }

    fn verbatim(&self, field: &str, text: &str, msg: &str, through: usize) -> Result<(), Refused> {
        match self.message(msg, through) {
            Some(message) if !text.is_empty() && message.text.contains(text) => Ok(()),
            Some(_) => Err(refused(
                "quote_mismatch",
                format!("{field} is not an exact substring of {msg}"),
            )),
            None => Err(refused(
                "marker_not_found",
                format!("{field} cites {msg}, which is not a parsed message"),
            )),
        }
    }

    fn ledger(&mut self, scope: &Scope) -> &mut (Ledger, Working) {
        let now = self.request.now.to_owned();
        self.ledgers.entry(scope.token()).or_insert_with(|| {
            let ledger = Ledger::new(scope.clone(), &now);
            let text = ledger.render().unwrap_or_else(|_| ledger.frontmatter());
            (
                ledger,
                Working {
                    text,
                    expected: None,
                    changed: true,
                },
            )
        })
    }

    #[allow(clippy::too_many_lines)]
    fn decision(
        &mut self,
        finding: &Finding,
        marker: &Marker,
        accepted: &mut Accepted,
        through: usize,
    ) -> Result<Option<Value>, Refused> {
        let input = finding.decision.as_ref();
        let expected = match marker.by {
            Author::User => DecidedBy::Human,
            Author::Agent => DecidedBy::Agent,
        };
        if let Some(input) = input {
            if input.orbit.is_some() {
                return Err(refused(
                    "invalid_decision",
                    "decision orbit is written only by Orbit decision ingestion",
                ));
            }
            if input.confirms.is_some() {
                return Err(refused(
                    "invalid_decision",
                    "decision confirms is written only by Orbit decision ingestion",
                ));
            }
            if let Some(Value::Array(items)) = &input.options {
                for item in items {
                    if let Some(item) = item.as_object()
                        && (item.contains_key("id") || item.contains_key("consequence"))
                    {
                        return Err(refused(
                            "invalid_decision",
                            "decision option id and consequence are written only by Orbit decision ingestion",
                        ));
                    }
                }
            }
        }
        let claimed = input.and_then(|d| d.decided_by).unwrap_or(expected);
        if claimed != expected {
            return Err(refused(
                "decided_by_mismatch",
                format!(
                    "decided_by must be {} for the {} marker",
                    match expected {
                        DecidedBy::Human => "human",
                        DecidedBy::Agent | DecidedBy::Model => "agent",
                    },
                    marker.by.as_str()
                ),
            ));
        }
        let scope = Scope::parse(&marker.scope).map_err(|_| {
            refused(
                "decision_invalid",
                format!("scope {} is not a ledger scope", marker.scope),
            )
        })?;
        let reference = format!("{}#T{} {}", self.scr_id, accepted.number, marker.msg);
        let at = |msg: &str| format!("{} {msg}", self.scr_id);
        let message = self
            .message(&marker.msg, through)
            .ok_or_else(|| refused("marker_not_found", "the marker's message is not parsed"))?;
        let decided_at = local(&message.at, self.request.zone)
            .map(rfc3339)
            .map_err(|_| refused("decision_invalid", "the marker's message has no valid time"))?;
        let mut decision = Decision {
            id: scope.entry_id(1),
            question: None,
            question_ref: None,
            question_by: None,
            options: None,
            chosen: None,
            quote: None,
            reference,
            because: None,
            scope: scope.token(),
            context: input.and_then(|d| d.context.clone()).unwrap_or_default(),
            decided_by: expected,
            decided_at,
            reverses: input.and_then(|d| d.reverses.clone()),
            orbit: None,
            same_as: input.and_then(|d| d.same_as.clone()),
            confirms: None,
            source: self.scr_id.clone(),
            private: self.private,
        };
        if !self.private {
            let input = input.ok_or_else(|| {
                refused(
                    "decision_invalid",
                    "a decision finding needs decision {question, options, chosen}",
                )
            })?;
            let question = input.question.clone().unwrap_or_default();
            match (&input.question_ref, input.question_by.as_deref()) {
                (Some(msg), None) => {
                    self.verbatim("decision question", &question, msg, through)?;
                    decision.question_ref = Some(at(msg));
                }
                (None, Some("scribe")) => decision.question_by = Some("scribe".into()),
                (Some(_) | None, Some(_) | None) => {
                    return Err(refused(
                        "decision_invalid",
                        "decision question needs question_ref (a message id) or question_by: scribe",
                    ));
                }
            }
            decision.question = Some(question);
            decision.options = Some(match &input.options {
                Some(Value::String(text)) if text == "unrecoverable" => Options::Unrecoverable,
                Some(Value::Array(items)) => {
                    let mut listed = Vec::new();
                    for item in items {
                        let span: Span = serde_json::from_value(item.clone()).map_err(|_| {
                            refused("decision_invalid", "options items are {text, msg}")
                        })?;
                        self.verbatim("decision option", &span.text, &span.msg, through)?;
                        listed.push(Cited {
                            text: span.text,
                            reference: at(&span.msg),
                            id: None,
                            consequence: None,
                        });
                    }
                    Options::Listed(listed)
                }
                Some(_) | None => {
                    return Err(refused(
                        "decision_invalid",
                        "decision options are \"unrecoverable\" or a list of {text, msg}",
                    ));
                }
            });
            let chosen = input.chosen.clone().unwrap_or_default();
            self.verbatim("decision chosen", &chosen, &marker.msg, through)?;
            decision.chosen = Some(chosen);
            decision.quote = Some(accepted.quote.clone());
            decision.because = match (&input.because, &marker.because) {
                (Some(span), _) => {
                    self.verbatim("decision because", &span.text, &span.msg, through)?;
                    Some(Cited {
                        text: span.text.clone(),
                        reference: at(&span.msg),
                        id: None,
                        consequence: None,
                    })
                }
                (None, Some(text)) => Some(Cited {
                    text: text.clone(),
                    reference: at(&marker.msg),
                    id: None,
                    consequence: None,
                }),
                (None, None) => None,
            };
        }
        if let Some(target) = &decision.reverses
            && !self
                .ledgers
                .values()
                .any(|(ledger, _)| ledger.entry(target).is_some())
        {
            return Err(refused(
                "decision_not_found",
                format!("reverses names {target}, which no decision ledger holds"),
            ));
        }
        if let Some(target) = &decision.same_as
            && !self
                .ledgers
                .values()
                .any(|(ledger, _)| ledger.entry(target).is_some())
        {
            return Err(refused(
                "decision_not_found",
                format!("same_as names {target}, which no decision ledger holds"),
            ));
        }
        if decision.same_as.is_none() && !self.private {
            let existing: Vec<Decision> = self
                .ledgers
                .values()
                .flat_map(|(ledger, _)| {
                    ledger
                        .entries
                        .iter()
                        .map(|entry| entry.decision.clone())
                        .collect::<Vec<_>>()
                })
                .collect();
            if let Some(target) = same_as_match(&decision, existing.iter()) {
                decision.same_as = Some(target);
            }
        }
        let (ledger, working) = self.ledger(&scope);
        decision.id = ledger.next_id();
        decision.validate().map_err(|error| {
            if let Error::Library { code, message } = error {
                refused(code, message)
            } else {
                refused("decision_invalid", error.to_string())
            }
        })?;
        let line = decision
            .line()
            .map_err(|_| refused("decision_invalid", "the entry could not be encoded"))?;
        let text = append_line(&working.text, &line);
        let parsed = Ledger::parse(&ledger.path, &text)
            .map_err(|_| refused("decision_invalid", "the ledger entry does not parse back"))?;
        let ledger_id = parsed.id();
        *ledger = parsed;
        working.text = text;
        working.changed = true;
        accepted.fields.push(("ledger".into(), decision.id.clone()));
        Ok(Some(json!({
            "id": decision.id,
            "ledger": ledger_id,
            "entry": format!("T{}", accepted.number),
            "decided_by": decision.decided_by,
            "private": decision.private,
        })))
    }

    fn preference(
        &mut self,
        finding: &Finding,
        marker: &Marker,
        tag: &ScribeTag,
        accepted: &mut Accepted,
    ) -> Result<(), Refused> {
        let entry = format!("T{}", accepted.number);
        if self.private {
            if finding.proposal.is_some() {
                return Err(refused(
                    "preference_withheld",
                    "a private session's preference is never proposed",
                ));
            }
            accepted.fields.push(("withheld".into(), "private".into()));
            return Ok(());
        }
        let matter = finding.matter.clone().unwrap_or_default();
        let statement = finding.statement.clone().unwrap_or_default();
        let message = self
            .stores
            .messages
            .iter()
            .find(|m| m.id == marker.msg)
            .ok_or_else(|| refused("marker_not_found", "the marker's message is not parsed"))?;
        let stated_at = local(&message.at, self.request.zone)
            .map(|at| at.format("%Y-%m-%d").to_string())
            .map_err(|_| {
                refused(
                    "preference_invalid",
                    "the marker's message has no valid time",
                )
            })?;
        let preference = Preference::new(
            matter.clone(),
            statement.clone(),
            vec![format!("{}#{entry}", self.scr_id)],
            stated_at,
            finding.review_by.clone(),
        )
        .map_err(|_| {
            refused(
                "preference_invalid",
                "a preference finding needs a kebab-case matter and a one-line statement",
            )
        })?;
        let scope = Scope::parse(&marker.scope)
            .map_err(|_| refused("preference_invalid", "the marker scope is not a page scope"))?;
        let page_id = preference_page_id(&scope);
        accepted.fields.push(("matter".into(), matter.clone()));
        let mut reason = match (marker.by, tag.mode) {
            (Author::Agent, TagMode::Propose | TagMode::Apply | TagMode::Append) => {
                Some("untrusted_candidate")
            }
            (Author::User, TagMode::Apply) if marker.large_turn => Some("paste_sized_turn"),
            (Author::User, TagMode::Apply) if !finding.conflicts.is_empty() => Some("conflict"),
            (Author::User, TagMode::Apply) => None,
            (Author::User, TagMode::Propose | TagMode::Append) => Some("propose_mode"),
        };
        if reason.is_none() {
            let path = preference_page_path(&scope);
            let current = self
                .pages
                .get(&page_id)
                .map(|w| w.text.clone())
                .or_else(|| {
                    self.stores
                        .pages
                        .iter()
                        .find(|p| p.path == path)
                        .and_then(|p| p.markdown.clone())
                });
            let base = current.clone().unwrap_or_else(|| new_page(&scope));
            match with_preference(&base, &page_id, &preference) {
                Ok(Some(text)) => {
                    let original = self
                        .stores
                        .pages
                        .iter()
                        .find(|p| p.path == path)
                        .and_then(|p| p.markdown.clone());
                    let working = self.pages.entry(page_id.clone()).or_insert(Working {
                        text: base,
                        expected: original,
                        changed: false,
                    });
                    working.text = text;
                    working.changed = true;
                    accepted.fields.push(("page".into(), page_id.clone()));
                    accepted.page = Some(page_id.clone());
                }
                Ok(None) => reason = Some("conflict"),
                Err(_) => reason = Some("page_invalid"),
            }
        }
        if let Some(reason) = reason {
            match &finding.proposal {
                Some(id) if token(id) => accepted.fields.push(("proposal".into(), id.clone())),
                Some(_) => {
                    return Err(refused("preference_invalid", "proposal must be one id"));
                }
                None => accepted.fields.push(("propose".into(), reason.into())),
            }
            accepted.proposal = Some(json!({
                "entry": entry, "cite": format!("{}#{entry}", self.scr_id), "msg": marker.msg,
                "by": marker.by, "scope": marker.scope, "page": page_id, "matter": matter,
                "statement": statement, "conflicts": finding.conflicts, "reason": reason,
                "proposal": finding.proposal,
            }));
        }
        Ok(())
    }
}

fn marker_view(marker: &Marker) -> Value {
    json!({"msg": marker.msg, "line": marker.line, "tag": marker.tag, "by": marker.by})
}

/// Plan one v2 pass. `validate_page(page_markdown, scr_markdown)` is the
/// caller's `office.knowledge-write` validation of an apply-mode page,
/// with the SCR as it will be written; a page it refuses falls back to a
/// proposal.
#[allow(clippy::too_many_lines)]
pub fn plan_pass(
    request: PassRequest<'_>,
    stores: PassStores<'_>,
    validate_page: &mut dyn FnMut(&str, &str) -> bool,
) -> Result<PassPlan> {
    let messages = stores.messages;
    let through = message_index(request.through)
        .filter(|index| *index < messages.len())
        .ok_or_else(|| invalid("scribe-append through must name a parsed message"))?;
    let existing = scr::find_session(stores.files, request.session_id)?;
    let cursor = match existing.and_then(|f| f.front.scribed_through.as_deref()) {
        None => None,
        Some(id) => Some(message_index(id).ok_or_else(|| {
            refusal(
                "scribe_file_invalid",
                "the SCR file's scribed_through is not a message id",
            )
        })?),
    };
    if cursor.is_some_and(|cursor| through < cursor) {
        return Err(invalid(
            "scribe-append through must not precede the SCR's scribed_through",
        ));
    }
    let scope = match (existing, &stores.parsed.scope) {
        (Some(file), _) => file.front.scope_token(),
        (None, Some(scope)) => scope.clone(),
        (None, None) => markers::session_scope(stores.parsed.project.as_deref(), None)?,
    };
    let private = stores.parsed.private || existing.is_some_and(|f| f.front.private());
    // Sections: the structured handoff validator and its budgets.
    let input: SectionsInput = serde_json::from_value(request.sections.clone()).map_err(|_| {
        invalid("scribe-append sections are {summary, goals[], outcome[], next[]} of strings")
    })?;
    let summary = if input.summary.trim_start().starts_with("(scribe)") {
        input.summary.trim().to_owned()
    } else {
        format!("(scribe) {}", input.summary.trim())
    };
    let strs = |items: &[String]| {
        items
            .iter()
            .map(|s| s.trim().to_owned())
            .collect::<Vec<_>>()
    };
    let sections = Sections {
        summary,
        goals: strs(&input.goals),
        outcome: strs(&input.outcome),
        next: strs(&input.next),
    };
    crate::domain::session::handoff::validate_scr_sections(
        &sections.summary,
        &refs(&sections.goals),
        &refs(&sections.outcome),
        &refs(&sections.next),
    )?;
    // Identity: fixed at creation, never renamed.
    let start_at = stores
        .parsed
        .started_at
        .clone()
        .or_else(|| messages.first().map(|m| m.at.clone()))
        .ok_or_else(|| invalid("the session has no start time"))?;
    let start = local(&start_at, request.zone)?;
    let (id, path) = match existing {
        Some(file) => (file.front.id.clone(), file.path.clone()),
        None => {
            let id = scr::scr_id(start, request.session_id);
            if stores
                .files
                .iter()
                .any(|f| f.front.id == id && f.front.session != request.session_id)
            {
                return Err(refusal(
                    "scribe_name_conflict",
                    "another session's SCR already has this session's name",
                ));
            }
            let path = scr::scr_path(start, &id);
            (id, path)
        }
    };
    let previous = match existing {
        Some(file) => file.front.previous.clone(),
        None => scr::latest_in_scope(stores.files, &scope, request.session_id)
            .map(|f| f.front.id.clone()),
    };
    // The markers this window may cite, recomputed without a model.
    let from = cursor.map_or(0, |cursor| cursor + 1);
    let (user, agent) = markers::detect(messages, from, Some(through), stores.tags, &scope);
    let found: Vec<&Marker> = user.iter().chain(agent.iter()).collect();
    let earlier: Vec<TagEntry> = existing.map(Scr::entries).unwrap_or_default();
    let mut next_number = existing.map_or(1, Scr::next_tag_number);
    let mut pass = Pass {
        request,
        stores,
        scr_id: id.clone(),
        private,
        ledgers: stores
            .ledgers
            .iter()
            .map(|(ledger, text)| {
                (
                    ledger.scope.token(),
                    (
                        ledger.clone(),
                        Working {
                            text: text.clone(),
                            expected: Some(text.clone()),
                            changed: false,
                        },
                    ),
                )
            })
            .collect(),
        pages: BTreeMap::new(),
    };
    let findings: Vec<Value> = match request.findings {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(invalid("scribe-append findings must be a list")),
    };
    let mut accepted: Vec<Accepted> = Vec::new();
    let mut plan = PassPlan {
        id: id.clone(),
        path: path.clone(),
        front: Front::default(),
        markdown: None,
        current: existing.map(|f| f.markdown.clone()),
        scope: scope.clone(),
        files: Vec::new(),
        added: Vec::new(),
        ledger: Vec::new(),
        applied: Vec::new(),
        needs_proposal: Vec::new(),
        withheld: Vec::new(),
        reported: Vec::new(),
        refused: Vec::new(),
        duplicates: Vec::new(),
    };
    let mut seen: BTreeSet<(String, usize)> = BTreeSet::new();
    for value in &findings {
        let finding: Finding = serde_json::from_value(value.clone()).map_err(|_| {
            invalid(
                "scribe-append findings are {msg, line, tag, by, quote, decision, matter, statement, conflicts, proposal, review_by, task, outcome}",
            )
        })?;
        let refuse = |plan: &mut PassPlan, why: Refused| {
            plan.refused.push(json!({
                "msg": finding.msg, "line": finding.line, "tag": finding.tag,
                "code": why.code, "reason": why.reason,
            }));
        };
        let marker = found
            .iter()
            .find(|m| m.msg == finding.msg && m.line == finding.line)
            .copied();
        // A retried finding whose marker is behind the cursor is still
        // recognized by its message and line text.
        let line_text = pass
            .message(&finding.msg, through)
            .and_then(|m| m.text.lines().nth(finding.line.checked_sub(1)?))
            .map(|line| line.trim().to_owned());
        let quote = finding
            .quote
            .clone()
            .or_else(|| marker.map(|m| m.quote.clone()))
            .or(line_text)
            .unwrap_or_default();
        if let Some(entry) = earlier
            .iter()
            .find(|e| e.msg == finding.msg && e.quote == quote)
        {
            plan.duplicates
                .push(json!({"msg": finding.msg, "line": finding.line, "entry": entry.id()}));
            continue;
        }
        let Some(marker) = marker else {
            refuse(
                &mut plan,
                refused(
                    "marker_not_found",
                    format!(
                        "no marker at {} line {} in this pass's messages",
                        finding.msg, finding.line
                    ),
                ),
            );
            continue;
        };
        if seen.contains(&(finding.msg.clone(), finding.line)) {
            plan.duplicates
                .push(json!({"msg": finding.msg, "line": finding.line, "entry": null}));
            continue;
        }
        if finding.tag != marker.tag {
            refuse(
                &mut plan,
                refused("tag_mismatch", "the finding's tag is not the marker's"),
            );
            continue;
        }
        if !marker.in_force {
            refuse(
                &mut plan,
                refused("tag_not_in_force", "the marker's tag is not defined"),
            );
            continue;
        }
        if finding
            .by
            .as_deref()
            .is_some_and(|by| by != marker.by.as_str())
        {
            refuse(
                &mut plan,
                refused(
                    "author_mismatch",
                    format!("by must be {} for this marker", marker.by.as_str()),
                ),
            );
            continue;
        }
        if let Err(why) = pass.verbatim("quote", &quote, &marker.msg, through) {
            refuse(&mut plan, why);
            continue;
        }
        let time = local(&marker.at, request.zone).map(line_time)?;
        let mut entry = Accepted {
            group: scr::group_heading(marker.tag.as_deref()),
            number: next_number,
            msg: marker.msg.clone(),
            line: marker.line,
            time,
            by: marker.by,
            scope: marker.scope.clone(),
            quote,
            fields: Vec::new(),
            page: None,
            proposal: None,
        };
        let definition = marker
            .tag
            .as_deref()
            .and_then(|name| stores.tags.iter().find(|t| t.name == name));
        let outcome: Result<(), Refused> = match definition.map(|t| (t, t.sink)) {
            Some((_, TagSink::DecisionsLedger)) => {
                match pass.decision(&finding, marker, &mut entry, through) {
                    Ok(Some(view)) => {
                        plan.ledger.push(view);
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(why) => Err(why),
                }
            }
            Some((tag, TagSink::PreferencePage)) => {
                pass.preference(&finding, marker, tag, &mut entry)
            }
            Some((_, TagSink::OrbitTask)) | None => match (marker.by, &finding.task) {
                (Author::Agent, Some(_)) => Err(refused(
                    "agent_marker_not_filed",
                    "an agent marker is reported, never filed",
                )),
                (Author::Agent, None) => {
                    entry.fields.push(("outcome".into(), "not_filed".into()));
                    plan.reported.push(marker_view(marker));
                    Ok(())
                }
                (Author::User, Some(task)) if task_id(task) => {
                    entry.fields.push(("task".into(), task.clone()));
                    Ok(())
                }
                (Author::User, Some(_)) => Err(refused(
                    "task_id_incomplete",
                    "task must be a complete Orbit task id such as ORB-<id>",
                )),
                (Author::User, None) => {
                    match (definition.is_some(), finding.outcome.as_deref()) {
                        (true, _) => entry.fields.push(("sink".into(), "failed".into())),
                        (false, Some(word)) if token(word) => {
                            entry.fields.push(("outcome".into(), word.to_owned()));
                        }
                        (false, Some(_) | None) => {
                            entry.fields.push(("outcome".into(), "reported".into()));
                        }
                    }
                    plan.reported.push(marker_view(marker));
                    Ok(())
                }
            },
            Some((_, TagSink::ScrOnly)) => Ok(()),
        };
        match outcome {
            Ok(()) => {
                seen.insert((finding.msg.clone(), finding.line));
                if private {
                    plan.withheld.push(marker.reference());
                }
                next_number += 1;
                accepted.push(entry);
            }
            Err(why) => refuse(&mut plan, why),
        }
    }
    // Title: a user title wins; otherwise the Summary's, on every pass.
    let front_title = |existing: Option<&Scr>| -> Result<(String, String)> {
        if let Some(file) = existing
            && file.front.title_by.as_deref() == Some("user")
            && let Some(title) = &file.front.title
            && request.title_by != Some("user")
        {
            return Ok((title.clone(), "user".into()));
        }
        let (title, by) = match (request.title, request.title_by) {
            (Some(title), Some("user")) => (title.trim().to_owned(), "user"),
            (Some(title), Some("scribe") | None) => (title_words(title), "scribe"),
            (None, Some("scribe") | None) => (title_words(&sections.summary), "scribe"),
            (_, Some(_)) => {
                return Err(invalid("scribe-append title_by is scribe or user"));
            }
        };
        if title.is_empty() || title.chars().any(char::is_control) || title.len() > 200 {
            return Err(invalid(
                "scribe-append title must be one nonempty line of at most 200 bytes",
            ));
        }
        Ok((title, by.into()))
    };
    let (title, title_by) = front_title(existing)?;
    let zone = request.zone;
    let parsed = stores.parsed;
    let front = Front {
        id: id.clone(),
        title: Some(title),
        title_by: Some(title_by),
        scope: if scope == "global" {
            "global".into()
        } else {
            "project".into()
        },
        project: scope.strip_prefix("project:").map(str::to_owned),
        session: request.session_id.to_owned(),
        started: rfc3339(start),
        updated: rfc3339(local(request.now, zone)?),
        previous,
        scribed_through: Some(request.through.to_owned()),
        provider: parsed.provider.clone(),
        model: parsed.model.clone(),
        host: parsed.host.clone(),
        cwd: parsed.cwd.clone(),
        branch: parsed.branch.clone(),
        transcript: Some(parsed.transcript.clone()),
        parsed: Some(parsed.parsed.clone()),
        sensitivity: if private { "private" } else { "normal" }.into(),
    };
    let render = |accepted: &[Accepted]| -> String {
        let mut groups: Vec<TagGroup> = existing.map(|f| f.tags.clone()).unwrap_or_default();
        for entry in accepted {
            let line = TagEntry::render(
                entry.number,
                &entry.msg,
                &entry.time,
                entry.by.as_str(),
                &entry.scope,
                &entry.quote,
                &entry.fields,
            );
            match groups.iter_mut().find(|g| g.heading == entry.group) {
                Some(group) => group.lines.push(line),
                None => groups.push(TagGroup {
                    heading: entry.group.clone(),
                    lines: vec![line],
                }),
            }
        }
        scr::sort_groups(&mut groups);
        scr::render(&front, &sections, &groups)
    };
    // Apply-mode pages: validated against the SCR as written; a page the
    // Library would refuse falls back to a proposal.
    let provisional = render(&accepted);
    let mut rejected: BTreeSet<String> = BTreeSet::new();
    for (page_id, working) in &pass.pages {
        if working.changed && !validate_page(&working.text, &provisional) {
            rejected.insert(page_id.clone());
        }
    }
    for entry in &mut accepted {
        if let Some(page_id) = entry.page.clone()
            && rejected.contains(&page_id)
        {
            entry.page = None;
            entry.fields.retain(|(key, _)| key != "page");
            entry.fields.push(("propose".into(), "page_invalid".into()));
            entry.proposal = Some(json!({
                "entry": format!("T{}", entry.number), "cite": format!("{id}#T{}", entry.number),
                "msg": entry.msg, "by": entry.by, "scope": entry.scope, "page": page_id,
                "reason": "page_invalid",
            }));
        }
    }
    for entry in &accepted {
        plan.added.push(json!({
            "id": format!("T{}", entry.number), "group": entry.group,
            "msg": entry.msg, "line": entry.line, "by": entry.by,
        }));
        if let Some(page) = &entry.page {
            plan.applied
                .push(json!({"entry": format!("T{}", entry.number), "page": page}));
        }
        if let Some(proposal) = &entry.proposal {
            plan.needs_proposal.push(proposal.clone());
        }
    }
    let markdown = render(&accepted);
    // A batch that refuses something and captures nothing writes nothing.
    let refused_only = accepted.is_empty() && !plan.refused.is_empty();
    let unchanged = existing.is_some_and(|file| {
        let mut same = front.clone();
        same.updated.clone_from(&file.front.updated);
        accepted.is_empty() && file.markdown == scr::render(&same, &sections, &file.tags)
    });
    if !(refused_only || unchanged) {
        plan.markdown = Some(markdown);
        for (_, (ledger, working)) in pass.ledgers {
            if working.changed {
                plan.files.push(PlannedFile {
                    path: ledger.path.clone(),
                    text: working.text,
                    expected: working.expected,
                });
            }
        }
        for (page_id, working) in pass.pages {
            if working.changed && !rejected.contains(&page_id) {
                plan.files.push(PlannedFile {
                    path: format!("{PREFERENCE_DIR}/{page_id}.md"),
                    text: working.text,
                    expected: working.expected,
                });
            }
        }
    } else {
        plan.ledger.clear();
        plan.applied.clear();
    }
    plan.front = front;
    Ok(plan)
}
