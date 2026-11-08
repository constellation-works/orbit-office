use serde::{Deserialize, Serialize};

use super::Scope;
use super::matching::parse_entry_id;
use super::scope::{LINE_TAG, invalid, refusal};
use super::types::{Cited, Context, DecidedBy, Options, OrbitInfo};
use crate::Result;

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

pub(super) fn offset_time(text: &str) -> bool {
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
pub(super) fn words(text: &str) -> String {
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
