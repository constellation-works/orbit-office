//! Rules-page sections and built-in scribe defaults.

use serde::Serialize;

use super::super::Rules;
use super::slug;

const USER_TAGS: &[&str] = &[
    "decision",
    "direction",
    "todo",
    "question",
    "reminder",
    "preference",
];
const AGENT_TAGS: &[&str] = &["agent-decision", "pending", "resolved", "incident"];
/// Matter slugs the spec names as defaults; rules may add more.
const DEFAULT_MATTERS: &[&str] = &["release-with-open-defects"];

/// Who may take a tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Authors {
    /// Daniel's own messages and `by: user` notes.
    User,
    /// Assistant messages and agent notes, with `by: <crew>`.
    Agent,
    /// Either.
    Any,
}

/// One allowed tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// The tag, without `#`.
    pub name: String,
    /// One-line meaning.
    pub meaning: String,
    /// Examples from the rules page.
    pub examples: Vec<String>,
    /// Who may take it; fixed for the built-in tags.
    pub authors: Authors,
}

/// Model and effort per scribe level, in `<model>@<effort>` form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScribeModels {
    /// Canned queries are model-free; `None` (the default) serializes as absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Model for free-form ask over a shortlist.
    pub ask: String,
    /// Model for the scribe pass.
    pub scribe: String,
}

fn default_models() -> ScribeModels {
    ScribeModels {
        query: None,
        ask: "haiku@low".to_owned(),
        scribe: "haiku@medium".to_owned(),
    }
}

fn valid_model(value: &str) -> bool {
    let Some((model, effort)) = value.split_once('@') else {
        return false;
    };
    !value[model.len() + 1..].contains('@') && token(model).is_some() && token(effort).is_some()
}

/// The rules the scribe agent follows and append enforces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScribeRules {
    /// `page` when `dom-scribe-rules` supplied any section, else `defaults`.
    pub source: &'static str,
    /// Allowed tags.
    pub tags: Vec<Tag>,
    /// Allowed topics.
    pub topics: Vec<String>,
    /// Allowed preference matters: the spec defaults plus the rules' list.
    pub matters: Vec<String>,
    /// Capture guidance for the agent.
    pub capture: Vec<String>,
    /// Models per level; the spec defaults when the page has no `Models` section.
    pub models: ScribeModels,
}

fn authors_of(name: &str) -> Authors {
    if USER_TAGS.contains(&name) {
        Authors::User
    } else if AGENT_TAGS.contains(&name) {
        Authors::Agent
    } else {
        Authors::Any
    }
}

fn default_tags() -> Vec<Tag> {
    [
        ("decision", "Daniel settles a choice."),
        ("direction", "A standing instruction from Daniel."),
        ("question", "An open question for later."),
        ("todo", "An action, optionally with due."),
        ("reminder", "A time-bound note to self."),
        (
            "fact",
            "A stated fact about hosts, people or configuration.",
        ),
        (
            "landed",
            "A PR merged or a task done, reported in the chat.",
        ),
        (
            "preference",
            "Daniel's standing answer to a class of question, with matter.",
        ),
        (
            "why",
            "The reason for an earlier decision, named by resolves: <SCR id>#L<n>.",
        ),
        (
            "agent-decision",
            "An orchestrator settles a choice within its delegated scope.",
        ),
        ("pending", "Work an orchestrator started or is waiting on."),
        (
            "resolved",
            "Closes an earlier pending line, named by resolves: <SCR id>#L<n>.",
        ),
        ("incident", "A failure or broken run an orchestrator hit."),
    ]
    .into_iter()
    .map(|(name, meaning)| Tag {
        name: name.into(),
        meaning: meaning.into(),
        examples: vec![],
        authors: authors_of(name),
    })
    .collect()
}

fn default_capture() -> Vec<String> {
    [
        "Only Daniel's messages and by: user notes take decision, direction, todo, question, reminder and preference.",
        "Assistant messages take landed and fact, and the agent tags agent-decision, pending, resolved and incident with by: <crew>.",
        "Skip status chatter and restated plans.",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn token(text: &str) -> Option<String> {
    let name = text.trim().trim_matches(['`', '*', '#']).trim();
    (!name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b)))
    .then(|| name.to_owned())
}

/// Topic token from a project name: lowercase, other characters as `-`.
#[must_use]
pub fn topic_of(project: &str) -> Option<String> {
    let slug = slug(project);
    (!slug.is_empty()).then_some(slug)
}

/// The rules from the page's `Tags`, `Topics`, `Matters`, `Capture` and
/// `Models` sections. A missing page, a private page or a missing section
/// uses the defaults; default topics are the project name and default
/// models are `haiku@low` (ask) and `haiku@medium` (scribe). Sections
/// from the old live scribe (`Note`, `Skip`, `Kinds`) are ignored.
#[must_use]
pub fn scribe_rules(page: Option<&Rules>, project: Option<&str>) -> ScribeRules {
    let mut rules = ScribeRules {
        source: "defaults",
        tags: default_tags(),
        topics: project.and_then(topic_of).into_iter().collect(),
        matters: DEFAULT_MATTERS.iter().map(|s| (*s).to_owned()).collect(),
        capture: default_capture(),
        models: default_models(),
    };
    let Some(body) = page.and_then(|page| page.body.as_deref()) else {
        return rules;
    };
    let mut section = String::new();
    let (mut tags, mut topics, mut capture, mut matters) =
        (Vec::<Tag>::new(), Vec::new(), Vec::new(), Vec::new());
    let mut models = default_models();
    let mut models_seen = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix('#') {
            section = heading
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .to_ascii_lowercase();
            continue;
        }
        let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };
        let nested = line.starts_with([' ', '\t']);
        match section.as_str() {
            "tags" if nested => {
                if let Some(tag) = tags.last_mut() {
                    tag.examples.push(item.trim().to_owned());
                }
            }
            "tags" => {
                let (name, meaning) = item
                    .split_once(':')
                    .or_else(|| item.split_once(" — "))
                    .or_else(|| item.split_once(" - "))
                    .unwrap_or((item, ""));
                if let Some(name) = token(name)
                    && !tags.iter().any(|tag| tag.name == name)
                {
                    tags.push(Tag {
                        authors: authors_of(&name),
                        name,
                        meaning: meaning.trim().to_owned(),
                        examples: vec![],
                    });
                }
            }
            "topics" if !nested => {
                let name = item.split_once(':').map_or(item, |(name, _)| name);
                if let Some(name) = token(name)
                    && !topics.contains(&name)
                {
                    topics.push(name);
                }
            }
            "matters" | "matter" if !nested => {
                let name = item.split_once(':').map_or(item, |(name, _)| name);
                if let Some(name) = token(name)
                    && !matters.contains(&name)
                {
                    matters.push(name);
                }
            }
            "capture" if !nested => capture.push(item.trim().to_owned()),
            "models" if !nested => {
                if let Some((level, value)) = item.split_once(':') {
                    let level = level.trim().trim_matches(['`', '*']).to_ascii_lowercase();
                    let value = value.trim().trim_matches(['`', '*']).trim();
                    match level.as_str() {
                        "query" if value.eq_ignore_ascii_case("none") => {
                            models.query = None;
                            models_seen = true;
                        }
                        "query" if valid_model(value) => {
                            models.query = Some(value.to_owned());
                            models_seen = true;
                        }
                        "ask" if valid_model(value) => {
                            models.ask = value.to_owned();
                            models_seen = true;
                        }
                        "scribe" if valid_model(value) => {
                            models.scribe = value.to_owned();
                            models_seen = true;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if !tags.is_empty() {
        rules.tags = tags;
        rules.source = "page";
    }
    if !topics.is_empty() {
        rules.topics = topics;
        rules.source = "page";
    }
    if !capture.is_empty() {
        rules.capture = capture;
        rules.source = "page";
    }
    if !matters.is_empty() {
        rules.source = "page";
    }
    if models_seen {
        rules.models = models;
        rules.source = "page";
    }
    for matter in matters {
        if !rules.matters.contains(&matter) {
            rules.matters.push(matter);
        }
    }
    rules
}
