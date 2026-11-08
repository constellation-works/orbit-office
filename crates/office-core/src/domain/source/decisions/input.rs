use serde::Deserialize;
use serde_json::Value;

use super::scope::{invalid, refusal};
use super::types::{Cited, Context, DecidedBy, Options};
use super::{Decision, Scope};
use crate::Result;

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
