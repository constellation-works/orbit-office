//! Decision ledgers (specs/decisions.md, "Decision ledger"): one append-only
//! file per scope at `sources/decisions/<scope>.md`, id `decisions-<scope>`,
//! one `dec-<scope>-<n>` entry per body line in the D13 schema.
//!
//! Pure: parsing, rendering, validation, `reversed_by` derivation, id and
//! alias resolution. The Library adapter reads and writes the files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::knowledge::{DocumentKind, DocumentRecord, LibraryLayer};
use crate::{Error, Result};

/// The source kind token and the directory under `sources/`.
pub const KIND: &str = "decisions";
/// The Library-relative directory holding every ledger file.
pub const LEDGER_DIR: &str = "sources/decisions";
/// The tag written on every ledger line.
const LINE_TAG: &str = "#decision";

fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    refusal("invalid_decision", message)
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A ledger's scope: `global` or `project:<name>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// `global`, stored at `sources/decisions/global.md`.
    Global,
    /// `project:<name>`, stored at `sources/decisions/<name>.md`.
    Project(String),
}

impl Scope {
    /// Parse `global` or `project:<name>`, where the name is a lowercase
    /// kebab-case slug other than `global`.
    pub fn parse(text: &str) -> Result<Self> {
        if text == "global" {
            return Ok(Self::Global);
        }
        match text.strip_prefix("project:") {
            Some(name) if is_slug(name) && name != "global" => Ok(Self::Project(name.into())),
            _ => Err(invalid(format!(
                "decision scope {text:?} must be global or project:<name> with a lowercase kebab-case name"
            ))),
        }
    }

    /// The canonical token (`global`, `project:<name>`).
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Project(name) => format!("project:{name}"),
        }
    }

    /// The slug naming the file and the ids (`global`, `<name>`).
    #[must_use]
    pub fn slug(&self) -> &str {
        match self {
            Self::Global => "global",
            Self::Project(name) => name,
        }
    }

    /// `decisions-<slug>`.
    #[must_use]
    pub fn ledger_id(&self) -> String {
        format!("{KIND}-{}", self.slug())
    }

    /// `sources/decisions/<slug>.md`.
    #[must_use]
    pub fn path(&self) -> String {
        format!("{LEDGER_DIR}/{}.md", self.slug())
    }

    /// `dec-<slug>-<n>`.
    #[must_use]
    pub fn entry_id(&self, number: usize) -> String {
        format!("dec-{}-{number}", self.slug())
    }
}

/// `office.capture` with no kind for input whose kind it cannot infer
/// (plain text): refused, naming the paste kinds and the `decision` input.
#[must_use]
pub fn source_kind_required() -> Error {
    refusal(
        "source_kind_required",
        "capture needs a kind for this input: use kind chat, email or doc for a paste; \
         a decision goes in the decision input, which appends to the decision ledger",
    )
}

/// `office.capture` with `kind: decisions`: ledgers take entries only
/// through the `decision` input.
#[must_use]
pub fn decision_input_required() -> Error {
    refusal(
        "decision_input_required",
        "decisions sources are ledgers: append an entry with the decision input, not kind decisions",
    )
}

/// Whether `relative` is a ledger file: exactly `sources/decisions/<slug>.md`.
#[must_use]
pub fn is_ledger_path(relative: &Path) -> bool {
    let mut components = relative.components();
    let names: Vec<&str> = components
        .by_ref()
        .map_while(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect();
    if components.next().is_some() {
        return false;
    }
    match names.as_slice() {
        ["sources", KIND, file] => file.strip_suffix(".md").is_some_and(is_slug),
        _ => false,
    }
}

/// Who made a decision (D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecidedBy {
    /// A user's decision.
    Human,
    /// An agent's decision.
    Agent,
    /// A decision model's answer, written only by the precedent feedback path.
    Model,
}

/// One verbatim span and the ref it was read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cited {
    /// The verbatim text.
    pub text: String,
    /// Where it was said.
    #[serde(rename = "ref")]
    pub reference: String,
    /// The option's stable id; set only by Orbit ingestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The option's one-line consequence; set only by Orbit ingestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consequence: Option<String>,
}

/// The model citation on an ingested entry answered by the model.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrbitModel {
    /// The assessment id.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "assessment")]
    pub assessment_id: Option<String>,
    /// The policy digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<String>,
    /// The model identity.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "identity")]
    pub model: Option<String>,
    /// The model's probability, kept as its written decimal text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_probability",
        serialize_with = "se_probability"
    )]
    pub probability: Option<String>,
    /// The precedents the model cited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "cited",
        alias = "precedents"
    )]
    pub cited_precedents: Option<Vec<String>>,
}

fn de_probability<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

fn se_probability<S>(value: &Option<String>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        None => serializer.serialize_none(),
        Some(text) => match text.parse::<serde_json::Number>() {
            Ok(number) => serializer.serialize_some(&Value::Number(number)),
            Err(_) => serializer.serialize_some(&Value::String(text.clone())),
        },
    }
}

/// The Orbit request an ingested entry records; ingestion only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrbitInfo {
    /// The answered request's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The request's gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// The request's category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The request's topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Who answered the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    /// The channel the answer arrived on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// The model citation, when the model answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<OrbitModel>,
}

/// The options considered: listed verbatim, or `"unrecoverable"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Value", into = "Value")]
pub enum Options {
    /// The turns do not state the options; never invented.
    Unrecoverable,
    /// Each option verbatim with its ref.
    Listed(Vec<Cited>),
}

impl TryFrom<Value> for Options {
    type Error = String;
    fn try_from(value: Value) -> std::result::Result<Self, String> {
        match value {
            Value::String(text) if text == "unrecoverable" => Ok(Self::Unrecoverable),
            Value::Array(_) => serde_json::from_value(value)
                .map(Self::Listed)
                .map_err(|_| "options items must be {text, ref}".into()),
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Object(_) => {
                Err("options must be \"unrecoverable\" or a list of {text, ref}".into())
            }
        }
    }
}

impl From<Options> for Value {
    fn from(options: Options) -> Self {
        match options {
            Options::Unrecoverable => Value::String("unrecoverable".into()),
            Options::Listed(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| {
                        let mut object =
                            serde_json::json!({"text": item.text, "ref": item.reference});
                        if let Some(id) = item.id {
                            object["id"] = Value::String(id);
                        }
                        if let Some(consequence) = item.consequence {
                            object["consequence"] = Value::String(consequence);
                        }
                        object
                    })
                    .collect(),
            ),
        }
    }
}

/// The session's Orbit assignment; each field null when unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Orbit task type.
    pub task_type: Option<String>,
    /// Orbit workspace id.
    pub workspace: Option<String>,
    /// Orbit task id.
    pub task: Option<String>,
    /// Risk tier.
    pub risk_tier: Option<String>,
}

/// One ledger entry in the D13 schema, in its written field order.
/// `reversed_by` is never written; see [`EntryView`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// `dec-<scope>-<n>`.
    pub id: String,
    /// What was being decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The message that asked the question verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_ref: Option<String>,
    /// `scribe` when the question is a restatement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_by: Option<String>,
    /// The options considered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
    /// The chosen option: a span of `quote`, up to case, punctuation and
    /// a word repeated in a row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<String>,
    /// The decision message's verbatim text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// Where the decision message is.
    #[serde(rename = "ref")]
    pub reference: String,
    /// The stated rationale, only when one was stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<Cited>,
    /// `global` or `project:<name>`.
    pub scope: String,
    /// The Orbit assignment.
    pub context: Context,
    /// Who decided.
    pub decided_by: DecidedBy,
    /// The decision message's time, RFC 3339 with offset.
    pub decided_at: String,
    /// The entry this one overturns.
    #[serde(default)]
    pub reverses: Option<String>,
    /// The Orbit request this entry ingests; ingestion only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orbit: Option<OrbitInfo>,
    /// The earlier entry recording the same decision from the other source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_as: Option<String>,
    /// The model entry this human entry confirms; ingestion only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirms: Option<String>,
    /// The evidence's source id.
    pub source: String,
    /// A reference-only entry from a private session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
}

/// Parse `dec-<slug>-<n>` into its slug and number.
#[must_use]
pub fn parse_entry_id(id: &str) -> Option<(&str, usize)> {
    let rest = id.strip_prefix("dec-")?;
    let (slug, number) = rest.rsplit_once('-')?;
    if !is_slug(slug) || number.is_empty() || !number.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number: usize = number.parse().ok()?;
    (number >= 1 && format!("dec-{slug}-{number}") == id).then_some((slug, number))
}

fn one_line(field: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() || text.contains(['\n', '\r']) {
        return Err(invalid(format!(
            "decision {field} must be one nonempty line"
        )));
    }
    Ok(())
}

fn nonempty(field: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(invalid(format!("decision {field} must not be empty")));
    }
    Ok(())
}

fn offset_time(text: &str) -> bool {
    let has_offset = text.ends_with('Z')
        || text
            .get(text.len().saturating_sub(6)..)
            .is_some_and(|tail| {
                let b = tail.as_bytes();
                b.len() == 6 && (b[0] == b'+' || b[0] == b'-') && b[3] == b':'
            });
    has_offset && crate::domain::metrics::timestamp(text).is_some()
}

/// Collapse runs of whitespace so a wrapped span still compares verbatim.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `quote` says `chosen`: its words appear in order in the quote
/// once case and punctuation are folded and a word said twice in a row
/// counts once, the noise a transcribed turn carries ("monthly, monthly.
/// Starting"). Anything the quote does not say still fails.
fn supports(quote: &str, chosen: &str) -> bool {
    let tokens = |text: &str| {
        let mut out: Vec<String> = Vec::new();
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
        {
            if out.last() != Some(&word) {
                out.push(word);
            }
        }
        out
    };
    let (quote, chosen) = (tokens(quote), tokens(chosen));
    !chosen.is_empty()
        && quote
            .windows(chosen.len())
            .any(|span| span == chosen.as_slice())
}

fn folded(text: &str) -> String {
    words(&text.to_lowercase())
}

/// Whether two chosen texts match after case and whitespace folding, or
/// one contains the other (the dedupe rule in decision-precedents.md).
#[must_use]
pub fn chosen_matches(left: &str, right: &str) -> bool {
    let left = folded(left);
    let right = folded(right);
    !left.is_empty() && (left == right || left.contains(&right) || right.contains(&left))
}

/// The one ingested entry `candidate` duplicates, if exactly one matches:
/// an entry carrying `orbit`, with the same `context.task`, a `decided_at`
/// within 30 minutes, and a `chosen` matching the candidate's chosen.
/// More than one candidate, or none, links nothing.
#[must_use]
pub fn same_as_match<'a>(
    candidate: &Decision,
    entries: impl IntoIterator<Item = &'a Decision>,
) -> Option<String> {
    let task = candidate.context.task.as_deref()?;
    let chosen = candidate.chosen.as_deref()?;
    let at = crate::domain::metrics::timestamp(&candidate.decided_at)?;
    let mut found: Option<&str> = None;
    for entry in entries {
        if entry.id == candidate.id || entry.orbit.is_none() {
            continue;
        }
        if entry.context.task.as_deref() != Some(task) {
            continue;
        }
        let Some(entry_at) = crate::domain::metrics::timestamp(&entry.decided_at) else {
            continue;
        };
        if (at - entry_at).abs() > 30 * 60 {
            continue;
        }
        let Some(entry_chosen) = entry.chosen.as_deref() else {
            continue;
        };
        // The candidate's chosen must match the ingested entry's chosen
        // answer: its `chosen`, or an option text that entry chose.
        let matches = chosen_matches(chosen, entry_chosen)
            || matches!(&entry.options, Some(Options::Listed(items))
                if items.iter().any(|item| chosen_matches(chosen, &item.text)
                    && chosen_matches(&item.text, entry_chosen)));
        if !matches {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(entry.id.as_str());
    }
    found.map(str::to_owned)
}

impl Decision {
    /// The parsed scope.
    pub fn parsed_scope(&self) -> Result<Scope> {
        Scope::parse(&self.scope)
    }

    /// Check the D13 rules that need no other entry or evidence. `id` must
    /// already be assigned.
    pub fn validate(&self) -> Result<()> {
        let scope = self.parsed_scope()?;
        match parse_entry_id(&self.id) {
            Some((slug, _)) if slug == scope.slug() => {}
            _ => {
                return Err(invalid(format!(
                    "decision id {:?} must be dec-{}-<n> for scope {}",
                    self.id,
                    scope.slug(),
                    scope.token()
                )));
            }
        }
        one_line("ref", &self.reference)?;
        one_line("source", &self.source)?;
        if !offset_time(&self.decided_at) {
            return Err(invalid(
                "decision decided_at must be an RFC 3339 time with an offset",
            ));
        }
        if let Some(reverses) = &self.reverses {
            // Whether it names an existing (so earlier, never this) entry is
            // the adapter's check on append: the id here may be provisional.
            if parse_entry_id(reverses).is_none() {
                return Err(invalid(
                    "decision reverses must name a dec-<scope>-<n> entry",
                ));
            }
        }
        for (field, value) in [("same_as", &self.same_as), ("confirms", &self.confirms)] {
            if let Some(value) = value
                && parse_entry_id(value).is_none()
            {
                return Err(invalid(format!(
                    "decision {field} must name a dec-<scope>-<n> entry"
                )));
            }
        }
        if let Some(orbit) = &self.orbit {
            for (field, value) in [
                ("request_id", &orbit.request_id),
                ("gate", &orbit.gate),
                ("category", &orbit.category),
                ("topic", &orbit.topic),
                ("answered_by", &orbit.answered_by),
                ("channel", &orbit.channel),
            ] {
                if let Some(value) = value {
                    one_line(&format!("orbit.{field}"), value)?;
                }
            }
            if let Some(model) = &orbit.model {
                for (field, value) in [
                    ("assessment_id", &model.assessment_id),
                    ("policy_digest", &model.policy_digest),
                    ("model", &model.model),
                ] {
                    if let Some(value) = value {
                        one_line(&format!("orbit.model.{field}"), value)?;
                    }
                }
                if let Some(probability) = &model.probability {
                    let parsed: std::result::Result<f64, _> = probability.trim().parse();
                    match parsed {
                        Ok(value) if (0.0..=1.0).contains(&value) => {}
                        _ => {
                            return Err(invalid(
                                "decision orbit.model.probability must be a number in 0..1",
                            ));
                        }
                    }
                }
                if let Some(cited) = &model.cited_precedents {
                    for item in cited {
                        one_line("orbit.model.cited_precedents", item)?;
                    }
                }
            }
        }
        for (field, value) in [
            ("task_type", &self.context.task_type),
            ("workspace", &self.context.workspace),
            ("task", &self.context.task),
            ("risk_tier", &self.context.risk_tier),
        ] {
            if let Some(value) = value {
                one_line(&format!("context.{field}"), value)?;
            }
        }
        if self.private {
            if self.question.is_some()
                || self.question_ref.is_some()
                || self.question_by.is_some()
                || self.options.is_some()
                || self.chosen.is_some()
                || self.quote.is_some()
                || self.because.is_some()
            {
                return Err(invalid(
                    "a private decision carries no question, options, chosen, quote or because",
                ));
            }
            return Ok(());
        }
        let question = self
            .question
            .as_deref()
            .ok_or_else(|| invalid("decision question is required"))?;
        nonempty("question", question)?;
        match (self.question_ref.as_deref(), self.question_by.as_deref()) {
            (Some(reference), None) => one_line("question_ref", reference)?,
            (None, Some("scribe")) => one_line("question", question)?,
            _ => {
                return Err(invalid(
                    "decision question needs question_ref (verbatim) or question_by: scribe (a restatement), not both",
                ));
            }
        }
        match &self.options {
            None => return Err(invalid("decision options are required")),
            Some(Options::Unrecoverable) => {}
            Some(Options::Listed(items)) => {
                if items.is_empty() {
                    return Err(invalid(
                        "decision options must list at least one option or be \"unrecoverable\"",
                    ));
                }
                for item in items {
                    nonempty("option text", &item.text)?;
                    one_line("option ref", &item.reference)?;
                    if let Some(id) = &item.id {
                        one_line("option id", id)?;
                    }
                    if let Some(consequence) = &item.consequence {
                        one_line("option consequence", consequence)?;
                    }
                }
            }
        }
        let quote = self
            .quote
            .as_deref()
            .ok_or_else(|| invalid("decision quote is required"))?;
        nonempty("quote", quote)?;
        let chosen = self
            .chosen
            .as_deref()
            .ok_or_else(|| invalid("decision chosen is required"))?;
        nonempty("chosen", chosen)?;
        if !supports(quote, chosen) {
            return Err(refusal(
                "quote_mismatch",
                "decision chosen must be a span of quote",
            ));
        }
        if let Some(because) = &self.because {
            nonempty("because text", &because.text)?;
            one_line("because ref", &because.reference)?;
        }
        Ok(())
    }

    /// Check every verbatim field against the text its ref names.
    /// `evidence` returns the text at a ref; a ref it cannot read is refused
    /// by the caller's error. A private entry has no verbatim fields, but its
    /// ref must still be readable.
    pub fn verify(&self, mut evidence: impl FnMut(&str) -> Result<String>) -> Result<()> {
        let mut check = |field: &str, text: &str, reference: &str| -> Result<()> {
            let found = evidence(reference)?;
            if words(&found).contains(&words(text)) {
                Ok(())
            } else {
                Err(refusal(
                    "quote_mismatch",
                    format!("decision {field} is not verbatim at {reference}"),
                ))
            }
        };
        if self.private {
            evidence(&self.reference)?;
            return Ok(());
        }
        if let Some(quote) = &self.quote {
            check("quote", quote, &self.reference)?;
        }
        if let (Some(question), Some(reference)) = (&self.question, &self.question_ref) {
            check("question", question, reference)?;
        }
        if let Some(Options::Listed(items)) = &self.options {
            for item in items {
                check("option", &item.text, &item.reference)?;
            }
        }
        if let Some(because) = &self.because {
            check("because", &because.text, &because.reference)?;
        }
        Ok(())
    }

    /// The one ledger line: `- <id> <decided_at> #decision <json>`.
    pub fn line(&self) -> Result<String> {
        let json =
            serde_json::to_string(self).map_err(|_| invalid("decision could not be encoded"))?;
        Ok(format!(
            "- {} {} {LINE_TAG} {json}",
            self.id, self.decided_at
        ))
    }
}

/// `office.capture`'s `decision` input: the D13 fields without `id`
/// (assigned on append) and without `reversed_by` (derived on read).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    question_ref: Option<String>,
    #[serde(default)]
    question_by: Option<String>,
    #[serde(default)]
    options: Option<Options>,
    #[serde(default)]
    chosen: Option<String>,
    #[serde(default)]
    quote: Option<String>,
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    because: Option<Cited>,
    scope: String,
    #[serde(default)]
    context: Option<Context>,
    decided_by: DecidedBy,
    decided_at: String,
    #[serde(default)]
    reverses: Option<String>,
    #[serde(default)]
    orbit: Option<Value>,
    #[serde(default)]
    same_as: Option<String>,
    #[serde(default)]
    confirms: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    private: bool,
}

/// Parse and validate a `decision` input. The id is provisional (the
/// scope's first entry id) until the adapter assigns the next one. Returns
/// the decision and the caller's `source` claim, which the caller checks
/// against the source its `ref` names. `decided_by: model` is refused: only
/// the precedent provider's feedback path writes it.
pub fn decision_from_input(value: &Value) -> Result<(Decision, Option<String>)> {
    let input: Input = serde_json::from_value(value.clone()).map_err(|error| {
        invalid(format!(
            "decision must be an object of the D13 fields (question, question_ref, question_by, options, chosen, quote, ref, because, scope, context, decided_by, decided_at, reverses, same_as, source, private): {error}"
        ))
    })?;
    if input.orbit.is_some() || input.confirms.is_some() {
        return Err(invalid(
            "decision orbit and confirms are written only by Orbit decision ingestion",
        ));
    }
    if let Some(Options::Listed(items)) = &input.options
        && items
            .iter()
            .any(|item| item.id.is_some() || item.consequence.is_some())
    {
        return Err(invalid(
            "decision option id and consequence are written only by Orbit decision ingestion",
        ));
    }
    if input.decided_by == DecidedBy::Model {
        return Err(refusal(
            "decided_by_mismatch",
            "decided_by model is written only by the decision model's feedback path; use human or agent",
        ));
    }
    let scope = Scope::parse(&input.scope)?;
    let decision = Decision {
        id: scope.entry_id(1),
        question: input.question,
        question_ref: input.question_ref,
        question_by: input.question_by,
        options: input.options,
        chosen: input.chosen,
        quote: input.quote,
        reference: input.reference,
        because: input.because,
        scope: scope.token(),
        context: input.context.unwrap_or_default(),
        decided_by: input.decided_by,
        decided_at: input.decided_at,
        reverses: input.reverses,
        orbit: None,
        same_as: input.same_as,
        confirms: None,
        source: String::from("pending"),
        private: input.private,
    };
    Ok((decision, input.source))
}

/// What a decision `ref` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceRef {
    /// `<source-id>#<anchor>`: a span of a Library source.
    Source {
        /// The source id as written.
        id: String,
        /// The anchor, without `#`.
        anchor: String,
    },
    /// `orbit://task/<task-id>#comment-<n>`: the n-th (1-based, oldest
    /// first) comment of an Orbit task.
    TaskComment {
        /// The task id.
        task: String,
        /// 1-based comment number.
        number: usize,
    },
}

impl EvidenceRef {
    /// Parse a ref; anything else is refused naming both forms.
    pub fn parse(reference: &str) -> Result<Self> {
        let unresolved = || {
            refusal(
                "decision_ref_unresolved",
                format!(
                    "decision ref {reference:?} must be <source-id>#<anchor> or orbit://task/<task-id>#comment-<n>"
                ),
            )
        };
        if let Some(rest) = reference.strip_prefix("orbit://task/") {
            let (task, comment) = rest.split_once('#').ok_or_else(unresolved)?;
            let number = comment
                .strip_prefix("comment-")
                .filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| *n >= 1)
                .ok_or_else(unresolved)?;
            if task.is_empty()
                || task.len() > 64
                || !task.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(unresolved());
            }
            return Ok(Self::TaskComment {
                task: task.into(),
                number,
            });
        }
        let (id, anchor) = reference.split_once('#').ok_or_else(unresolved)?;
        let citation = crate::domain::knowledge::parse_citation(reference)
            .filter(|c| c.kind == DocumentKind::Source && !c.id.contains('/'))
            .ok_or_else(unresolved)?;
        if citation.anchor.as_deref() != Some(anchor) {
            return Err(unresolved());
        }
        Ok(Self::Source {
            id: id.into(),
            anchor: anchor.into(),
        })
    }

    /// The `source` value an entry citing this ref records: the source id,
    /// or `orbit-task:<task-id>` for a task comment.
    #[must_use]
    pub fn source_id(&self) -> String {
        match self {
            Self::Source { id, .. } => id.clone(),
            Self::TaskComment { task, .. } => format!("orbit-task:{task}"),
        }
    }
}

/// A ledger entry and the body line it occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 1-based body line.
    pub line: usize,
    /// The entry.
    pub decision: Decision,
}

/// A parsed ledger file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    /// Library-relative path.
    pub path: String,
    /// The scope.
    pub scope: Scope,
    /// `created_at` from the front matter.
    pub created_at: String,
    /// Migrated source ids mapped to their entry ids.
    pub moved_sources: BTreeMap<String, String>,
    /// Entries in line order.
    pub entries: Vec<LedgerEntry>,
}

/// A ledger whose file is malformed is refused, naming the file.
fn malformed(path: &str, reason: &str) -> Error {
    refusal(
        "decision_ledger_invalid",
        format!("decision ledger {path} is malformed: {reason}"),
    )
}

impl Ledger {
    /// An empty ledger for `scope`.
    #[must_use]
    pub fn new(scope: Scope, created_at: &str) -> Self {
        Self {
            path: scope.path(),
            scope,
            created_at: created_at.into(),
            moved_sources: BTreeMap::new(),
            entries: Vec::new(),
        }
    }

    /// The ledger's id, `decisions-<slug>`.
    #[must_use]
    pub fn id(&self) -> String {
        self.scope.ledger_id()
    }

    /// The ledger's title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Decisions ({})", self.scope.token())
    }

    /// Parse a ledger file. The path must be a ledger path and agree with
    /// the front matter; every body line must be one entry whose id is the
    /// next in sequence.
    pub fn parse(path: &str, markdown: &str) -> Result<Self> {
        use crate::domain::knowledge::frontmatter;
        if !is_ledger_path(Path::new(path)) {
            return Err(malformed(path, "not a sources/decisions/<scope>.md path"));
        }
        let (header, body) =
            frontmatter::split(markdown).map_err(|_| malformed(path, "no front matter"))?;
        let fields =
            frontmatter::parse(header).map_err(|_| malformed(path, "unreadable front matter"))?;
        let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let text = |key: &str| field(key).and_then(Value::as_str).map(str::to_owned);
        if text("kind").as_deref() != Some(KIND) {
            return Err(malformed(path, "kind is not decisions"));
        }
        let scope = Scope::parse(&text("scope").unwrap_or_default())
            .map_err(|_| malformed(path, "scope is missing or invalid"))?;
        if scope.path() != path || text("id") != Some(scope.ledger_id()) {
            return Err(malformed(path, "id, scope and path disagree"));
        }
        let mut moved_sources = BTreeMap::new();
        match field("moved_sources") {
            None | Some(Value::Null) => {}
            Some(Value::Object(map)) => {
                for (old, new) in map {
                    let new = new
                        .as_str()
                        .ok_or_else(|| malformed(path, "moved_sources values must be entry ids"))?;
                    moved_sources.insert(old.clone(), new.to_owned());
                }
            }
            Some(_) => return Err(malformed(path, "moved_sources must be a mapping")),
        }
        let mut entries = Vec::new();
        for (index, line) in body.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let decision = parse_line(line).ok_or_else(|| {
                malformed(path, &format!("line {} is not a decision entry", index + 1))
            })?;
            if decision.id != scope.entry_id(entries.len() + 1) || decision.scope != scope.token() {
                return Err(malformed(
                    path,
                    &format!("line {} is out of sequence or scope", index + 1),
                ));
            }
            entries.push(LedgerEntry {
                line: index + 1,
                decision,
            });
        }
        Ok(Self {
            path: path.into(),
            scope,
            created_at: text("created_at").unwrap_or_default(),
            moved_sources,
            entries,
        })
    }

    /// The next entry id.
    #[must_use]
    pub fn next_id(&self) -> String {
        self.scope.entry_id(self.entries.len() + 1)
    }

    /// The front matter, rendered.
    #[must_use]
    pub fn frontmatter(&self) -> String {
        let quote = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
        let mut out = String::from("---\n");
        out.push_str(&format!("id: {}\n", self.id()));
        out.push_str(&format!("kind: {KIND}\n"));
        out.push_str(&format!("title: {}\n", quote(&self.title())));
        out.push_str(&format!("scope: {}\n", quote(&self.scope.token())));
        out.push_str(&format!("created_at: {}\n", quote(&self.created_at)));
        out.push_str("sensitivity: normal\n");
        if self.moved_sources.is_empty() {
            out.push_str("moved_sources: {}\n");
        } else {
            out.push_str("moved_sources:\n");
            for (old, new) in &self.moved_sources {
                out.push_str(&format!("  {old}: {new}\n"));
            }
        }
        out.push_str("---\n");
        out
    }

    /// The whole file: front matter, then one line per entry. Entries are
    /// re-rendered canonically, so callers append to the existing body
    /// instead when one exists ([`append_line`]).
    pub fn render(&self) -> Result<String> {
        let mut out = self.frontmatter();
        for entry in &self.entries {
            out.push_str(&entry.decision.line()?);
            out.push('\n');
        }
        Ok(out)
    }

    /// The entry with this id, if any.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&LedgerEntry> {
        self.entries.iter().find(|entry| entry.decision.id == id)
    }
}

/// Parse one `- <id> <time> #decision <json>` line.
fn parse_line(line: &str) -> Option<Decision> {
    let rest = line.strip_prefix("- ")?;
    let (id, rest) = rest.split_once(' ')?;
    let (_time, rest) = rest.split_once(' ')?;
    let json = rest.strip_prefix(LINE_TAG)?.strip_prefix(' ')?;
    let decision: Decision = serde_json::from_str(json).ok()?;
    (decision.id == id).then_some(decision)
}

/// `existing` (a ledger file's full text) with one more line, keeping every
/// earlier byte.
#[must_use]
pub fn append_line(existing: &str, line: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

/// An entry as read: the written fields plus the derived ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryView {
    /// The written entry.
    #[serde(flatten)]
    pub decision: Decision,
    /// The first later entry whose `reverses` names this one; never written.
    pub reversed_by: Option<String>,
    /// The ledger's id.
    pub ledger: String,
    /// The entry's 1-based line in the ledger body.
    pub line: usize,
}

/// Every entry across `ledgers`, with `reversed_by` derived from every
/// ledger's `reverses`. Ledger order, then line order.
#[must_use]
pub fn views(ledgers: &[Ledger]) -> Vec<EntryView> {
    let mut reversed: BTreeMap<&str, &str> = BTreeMap::new();
    for ledger in ledgers {
        for entry in &ledger.entries {
            if let Some(target) = entry.decision.reverses.as_deref() {
                reversed.entry(target).or_insert(entry.decision.id.as_str());
            }
        }
    }
    ledgers
        .iter()
        .flat_map(|ledger| {
            let reversed = &reversed;
            ledger.entries.iter().map(move |entry| EntryView {
                decision: entry.decision.clone(),
                reversed_by: reversed
                    .get(entry.decision.id.as_str())
                    .map(|id| (*id).to_owned()),
                ledger: ledger.id(),
                line: entry.line,
            })
        })
        .collect()
}

/// What an id names among the ledgers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// A ledger by its `decisions-<scope>` id.
    Ledger(&'a Ledger),
    /// One entry, by its `dec-` id or a migrated source's former id
    /// (`moved_from`).
    Entry {
        /// The ledger holding it.
        ledger: &'a Ledger,
        /// The entry.
        entry: &'a LedgerEntry,
        /// The migrated source id the lookup used, if any.
        moved_from: Option<String>,
    },
}

/// Resolve a ledger id, an entry id or a migrated source id.
#[must_use]
pub fn resolve<'a>(ledgers: &'a [Ledger], id: &str) -> Option<Resolved<'a>> {
    if let Some(ledger) = ledgers.iter().find(|ledger| ledger.id() == id) {
        return Some(Resolved::Ledger(ledger));
    }
    if parse_entry_id(id).is_some() {
        return ledgers.iter().find_map(|ledger| {
            ledger.entry(id).map(|entry| Resolved::Entry {
                ledger,
                entry,
                moved_from: None,
            })
        });
    }
    ledgers.iter().find_map(|ledger| {
        let target = ledger.moved_sources.get(id)?;
        ledger.entry(target).map(|entry| Resolved::Entry {
            ledger,
            entry,
            moved_from: Some(id.to_owned()),
        })
    })
}

/// The document record a citation check sees for an entry: id as cited
/// (entry or former source id), the ledger's path, and the entry's verbatim
/// `quote` as the body, so a former `src-…#L<n>` anchor still names a line of
/// the text it cited. A private entry has an empty body.
#[must_use]
pub fn entry_record(ledger: &Ledger, entry: &LedgerEntry, cited_id: &str) -> DocumentRecord {
    let decision = &entry.decision;
    DocumentRecord {
        layer: LibraryLayer::Own,
        shared_by: None,
        id: cited_id.into(),
        kind: DocumentKind::Source,
        title: decision
            .question
            .clone()
            .unwrap_or_else(|| decision.id.clone()),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".into(),
        shareable: false,
        path: ledger.path.clone(),
        body: decision.quote.clone().unwrap_or_default(),
        cites: Vec::new(),
        related: Vec::new(),
        template: None,
        date: decision.decided_at.get(..10).map(str::to_owned),
    }
}

/// Whether a record stands for one ledger entry (cited bare), rather than a
/// ledger or an ordinary source.
#[must_use]
pub fn is_entry_record(record: &DocumentRecord) -> bool {
    is_ledger_path(Path::new(&record.path)) && !record.id.starts_with("decisions-")
}

/// The scope a migrated note goes to: a `project:<name>` or
/// `scope:project:<name>` tag names a project, otherwise `global`.
#[must_use]
pub fn note_scope(tags: &[String]) -> Scope {
    tags.iter()
        .find_map(|tag| {
            let tag = tag.strip_prefix("scope:").unwrap_or(tag);
            tag.starts_with("project:")
                .then(|| Scope::parse(tag).ok())
                .flatten()
        })
        .unwrap_or(Scope::Global)
}

/// A retired `note` source, as the migration reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteSource {
    /// Its `src-…` id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its body, verbatim.
    pub body: String,
    /// The ledger scope it moves to ([`note_scope`]).
    pub scope: Scope,
    /// `occurred_at`, else `captured_at`, else `fallback`.
    pub decided_at: String,
    /// Whether the note is private (left in place by the migration).
    pub private: bool,
}

/// Read a `source.md` as a retired note; `None` for any other kind.
#[must_use]
pub fn note_source(path: &str, markdown: &str, fallback: &str) -> Option<NoteSource> {
    use crate::domain::knowledge::frontmatter;
    let meta = super::parse_source_meta(markdown);
    if meta.kind != "note" || meta.id.is_empty() {
        return None;
    }
    let fields = frontmatter::split(markdown)
        .and_then(|(header, _)| frontmatter::parse(header))
        .unwrap_or_default();
    let decided_at = ["occurred_at", "captured_at"]
        .iter()
        .find_map(|key| {
            fields
                .iter()
                .find(|(name, _)| name == key)
                .and_then(|(_, value)| value.as_str())
                .filter(|value| offset_time(value))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| fallback.to_owned());
    let record = DocumentRecord::from_markdown(Path::new(path), markdown);
    Some(NoteSource {
        id: meta.id,
        title: meta.title,
        body: super::source_body(markdown),
        scope: note_scope(&record.tags),
        decided_at,
        private: meta.sensitivity == "private",
    })
}

/// The ledger entry a retired `note` source becomes (specs/decisions.md,
/// "Retiring `sources/note`"): the title restated as the question, options
/// unrecoverable, the body verbatim as the quote, its first line as the
/// chosen span, `decided_by: human` and `source` set to the note's id.
pub fn note_entry(
    scope: &Scope,
    id: &str,
    source_id: &str,
    title: &str,
    body: &str,
    decided_at: &str,
) -> Result<Decision> {
    let quote = body.trim_end_matches('\n').to_string();
    let chosen = quote
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string();
    let lines = quote.lines().count().max(1);
    let question = if title.trim().is_empty() {
        chosen.clone()
    } else {
        title.trim().replace(['\n', '\r'], " ")
    };
    let decision = Decision {
        id: id.into(),
        question: Some(question),
        question_ref: None,
        question_by: Some("scribe".into()),
        options: Some(Options::Unrecoverable),
        chosen: Some(chosen),
        quote: Some(quote),
        reference: if lines == 1 {
            format!("{source_id}#L1")
        } else {
            format!("{source_id}#L1-{lines}")
        },
        because: None,
        scope: scope.token(),
        context: Context::default(),
        decided_by: DecidedBy::Human,
        decided_at: decided_at.into(),
        reverses: None,
        orbit: None,
        same_as: None,
        confirms: None,
        source: source_id.into(),
        private: false,
    };
    decision.validate()?;
    Ok(decision)
}

/// One decision ledger file to write, compare-and-set: `expected` is the
/// text read before planning (`None`: the file must not exist yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedFile {
    /// Library-relative path.
    pub path: String,
    /// Full new text.
    pub text: String,
    /// The text planned against.
    pub expected: Option<String>,
}
