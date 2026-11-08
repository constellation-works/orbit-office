//! Decision and direction cue classification and evidence entries.

use super::super::format_conversation_entry;
use super::types::{Line, Request, Selection, Session, invalid};
use crate::Result;

fn ambiguous(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains('?')
        || [
            "maybe", "perhaps", "thinking", "might", "could", "not sure", "if ", "consider",
        ]
        .iter()
        .any(|s| lower.contains(s))
}
const DECISION_CUES: &[&str] = &["decision:", "i decided ", "we decided ", "i choose "];
const DIRECTION_CUES: &[&str] = &[
    "direction:",
    "always ",
    "never ",
    "please ",
    "we need to ",
    "i want you to ",
];

/// Text after a leading `Note:`/`Scribe:` command prefix, if this is one.
fn note_remainder(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    ["note:", "scribe:"]
        .iter()
        .find(|prefix| lower.starts_with(*prefix))
        .map(|prefix| text[prefix.len()..].trim_start())
}

/// Deterministic kind for a `Note:`/`Scribe:` command: a decision or
/// direction cue in the text after the prefix wins, otherwise a fact.
/// Commands are captured without model judgment, so ambiguity in the
/// remainder does not demote them the way ordinary lines are demoted.
pub fn note_kind(text: &str) -> Option<&'static str> {
    let rest = note_remainder(text)?;
    let lower = rest.to_ascii_lowercase();
    if DECISION_CUES.iter().any(|p| lower.starts_with(p)) {
        Some("decision")
    } else if DIRECTION_CUES.iter().any(|p| lower.starts_with(p)) {
        Some("direction")
    } else {
        Some("fact")
    }
}

fn explicit_kind(line: &Line) -> &'static str {
    let lower = line.text.to_ascii_lowercase();
    if line.role == "user"
        && let Some(kind) = note_kind(&line.text)
    {
        return kind;
    }
    if ambiguous(&line.text) {
        return "open";
    }
    if line.role == "assistant" {
        return if lower.starts_with("action:") {
            "action"
        } else {
            "open"
        };
    }
    if DECISION_CUES.iter().any(|p| lower.starts_with(p)) {
        "decision"
    } else if DIRECTION_CUES.iter().any(|p| lower.starts_with(p)) {
        "direction"
    } else if lower.starts_with("fact:") {
        "fact"
    } else {
        "open"
    }
}
/// Replies that accept a yes/no question, matched as a leading phrase.
const AFFIRMATIVES: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "sure",
    "ok",
    "okay",
    "do it",
    "go ahead",
    "go for it",
    "sounds good",
    "ship it",
    "please do",
    "lgtm",
    "agreed",
    "approved",
];
/// Replies that decline a yes/no question, matched as a leading phrase.
const NEGATIVES: &[&str] = &[
    "no",
    "nope",
    "not now",
    "not yet",
    "no thanks",
    "don't",
    "do not",
];
/// Words that qualify a reply, so it no longer simply answers the question.
const QUALIFIERS: &[&str] = &["but", "except", "unless", "instead", "wait"];
/// Words that turn an affirmative into something else.
const NEGATIONS: &[&str] = &["no", "not", "nope", "don't", "dont", "never"];
/// Question words that make a question open rather than yes/no.
const WH_WORDS: &[&str] = &[
    "what", "which", "where", "when", "who", "whom", "whose", "why", "how",
];

fn normalized(text: &str) -> String {
    text.trim().to_lowercase().replace('\u{2019}', "'")
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect()
}

/// True when `text` begins with `phrase` followed by a non-word character.
fn leads_with(text: &str, phrase: &str) -> bool {
    text.strip_prefix(phrase).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '\''))
    })
}

/// Option labels a question offers: `(a)`, `a)`, `(1)`, `1)` or `option a`.
fn option_labels(question: &str) -> Vec<String> {
    let lower = normalized(question);
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let mut labels = Vec::new();
    let short = |label: &str| {
        (1..=2).contains(&label.len()) && label.chars().all(|c| c.is_ascii_alphanumeric())
    };
    for (index, token) in tokens.iter().enumerate() {
        let token = token.trim_end_matches([',', ';', ':', '.', '?']);
        let bare = token.strip_prefix('(').unwrap_or(token);
        if let Some(label) = bare.strip_suffix(')')
            && short(label)
        {
            labels.push(label.to_string());
        }
        if (token == "option" || token == "options")
            && let Some(next) = tokens.get(index + 1)
        {
            let label = next.trim_end_matches([',', ';', ':', '.', '?']);
            if short(label) {
                labels.push(label.to_string());
            }
        }
    }
    labels.sort();
    labels.dedup();
    labels
}

/// The rendered answer text when `reply` answers `question` under the
/// deterministic checks (specs/sessions.md, Answers to questions):
/// `question` is the nearest assistant line before the reply with no user
/// line between them and ends in `?`; the reply carries no uncertainty
/// marker and is not a question; it is an affirmative or negative to a
/// yes/no question, or an option label the question offered.
fn answer(session: &Session, reply: usize, question: usize) -> Option<String> {
    let asked = session.lines.get(question)?;
    let said = session.lines.get(reply)?;
    // Lines are numbered by position, so the nearest earlier assistant line
    // with no user line in between is exactly the previous line.
    // Private sessions stay on the local conservative path, which never
    // has crew answers.
    if session.sensitivity == "private"
        || question.checked_add(1) != Some(reply)
        || asked.role != "assistant"
        || said.role != "user"
        || !asked.text.trim_end().ends_with('?')
        || ambiguous(&said.text)
    {
        return None;
    }
    let reply_text = normalized(&said.text);
    let reply_words = words(&reply_text);
    if reply_words.iter().any(|word| QUALIFIERS.contains(word)) {
        return None;
    }
    let labels = option_labels(&asked.text);
    let verb = if labels.is_empty() {
        let question_text = normalized(&asked.text);
        let question_words = words(&question_text);
        let yes_no = question_words
            .first()
            .is_some_and(|first| !WH_WORDS.contains(first))
            && !question_words.contains(&"or");
        if !yes_no {
            return None;
        }
        if AFFIRMATIVES.iter().any(|p| leads_with(&reply_text, p))
            && !reply_words.iter().any(|word| NEGATIONS.contains(word))
        {
            "answering"
        } else if NEGATIVES.iter().any(|p| leads_with(&reply_text, p)) {
            "declining"
        } else {
            return None;
        }
    } else {
        let choice = reply_text.trim_end_matches(['.', '!']);
        let choice = choice.strip_prefix("option ").unwrap_or(choice);
        let choice = choice.strip_prefix('(').unwrap_or(choice);
        let choice = choice.strip_suffix(')').unwrap_or(choice);
        if !labels.iter().any(|label| label == choice) {
            return None;
        }
        "answering"
    };
    Some(format!(
        "\"{}\" ({verb} #L{question}: \"{}\")",
        said.text, asked.text
    ))
}

/// Validate model selections and render exact evidence lines. An unclear or
/// non-user decision/direction becomes open even if a model misclassifies it.
pub fn entries(session: &Session, request: &Request) -> Result<Vec<String>> {
    if request.selections.is_some()
        && request.expected_sha256.as_deref() != Some(&session.transcript_sha256)
    {
        return Err(invalid(
            "transcript changed since preview; read a fresh preview",
        ));
    }
    if let Some(from_line) = request.from_line
        && let Some(selections) = &request.selections
        && selections.iter().any(|s| s.line < from_line)
    {
        return Err(invalid(
            "selections before from_line name read-only context and cannot be selected",
        ));
    }
    // `Note:`/`Scribe:` commands are captured without model judgment: add
    // any the crew did not select, in the range this pass considers.
    let from_line = request.from_line.unwrap_or(0);
    let augmented;
    let defaults;
    let selections = match &request.selections {
        Some(selections) => {
            let chosen: std::collections::BTreeSet<usize> =
                selections.iter().map(|s| s.line).collect();
            let mut all = selections.clone();
            for line in &session.lines {
                if line.line >= from_line
                    && line.role == "user"
                    && !chosen.contains(&line.line)
                    && let Some(kind) = note_kind(&line.text)
                {
                    all.push(Selection {
                        line: line.line,
                        kind: kind.into(),
                        answers: None,
                    });
                }
            }
            augmented = all;
            &augmented
        }
        None => {
            defaults = session
                .lines
                .iter()
                .filter(|l| l.role == "user" || l.text.to_ascii_lowercase().starts_with("action:"))
                .map(|l| Selection {
                    line: l.line,
                    kind: explicit_kind(l).into(),
                    answers: None,
                })
                .collect::<Vec<_>>();
            &defaults
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    for selection in selections {
        if !["decision", "direction", "action", "fact", "open"].contains(&selection.kind.as_str())
            || !seen.insert(selection.line)
        {
            return Err(invalid(
                "selections need unique lines and a supported extraction kind",
            ));
        }
        let line = session
            .lines
            .get(selection.line)
            .ok_or_else(|| invalid("selection line is outside the transcript"))?;
        let mut kind = selection.kind.as_str();
        let mut text = line.text.clone();
        // A command's kind is fixed by its cue, not by the crew's choice.
        let forced = (line.role == "user")
            .then(|| note_kind(&line.text))
            .flatten();
        if let Some(forced) = forced {
            kind = forced;
        } else if kind == "decision"
            && explicit_kind(line) != kind
            && let Some(answer) = selection
                .answers
                .and_then(|question| answer(session, selection.line, question))
        {
            text = answer;
        } else if (["decision", "direction"].contains(&kind) && explicit_kind(line) != kind)
            || (kind == "fact" && (line.role != "user" || ambiguous(&line.text)))
            || (kind == "action" && line.role != "assistant")
        {
            kind = "open";
        }
        ordered.push((
            selection.line,
            format_conversation_entry(&line.timestamp, kind, &text)?,
        ));
    }
    ordered.sort_by_key(|(line, _)| *line);
    Ok(ordered.into_iter().map(|(_, text)| text).collect())
}
