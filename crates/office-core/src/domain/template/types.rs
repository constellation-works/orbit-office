//! Template rule types and page validation.

use office_common::Sensitivity;
use serde_json::Value;

use crate::domain::knowledge::DocumentKind;
use crate::{Error, Result};

use super::rules::{invalid, is_slug, parse_rules, rule_list, rule_string, split_frontmatter};
use super::sections::populated_sections;

/// Knowledge layer allowed by a template's `type` rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateKind {
    /// An About page.
    Domain,
    /// A How-to page.
    Runbook,
}

impl TemplateKind {
    /// Canonical frontmatter token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Runbook => "runbook",
        }
    }

    fn document_kind(self) -> DocumentKind {
        match self {
            Self::Domain => DocumentKind::Domain,
            Self::Runbook => DocumentKind::Runbook,
        }
    }
}

/// How automatic attachment selects pages using a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachPolicy {
    /// Include current pages on every task.
    Always,
    /// Match the source's participants against person names and aliases.
    Participants,
    /// Match client, project or topic tags.
    Tagged,
    /// Match glossary terms in the source.
    Matched,
    /// No automatic policy attachment (runbooks may still be named by a task).
    Never,
}

/// Who may carry a runbook out, from its `performable` rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Performable {
    /// The agent may follow the runbook up to submission.
    Agent,
    /// Only the user performs it (the default when the rule is absent).
    #[default]
    You,
}

impl Performable {
    /// Canonical frontmatter token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::You => "you",
        }
    }
}

/// How a performed runbook's result is submitted, from its `submission.via`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionVia {
    /// Produce a draft artifact; the user submits it.
    Draft,
    /// Produce a gateway draft for the named connection; approval sends it.
    Gateway,
}

impl SubmissionVia {
    /// Canonical frontmatter token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Gateway => "gateway",
        }
    }
}

/// Where a performed runbook's result goes, from its `submission` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    /// Target system name, for example a ticketing system.
    pub system: String,
    /// Submission channel.
    pub via: SubmissionVia,
    /// Gateway connection name; required when `via` is `gateway`.
    pub connection: Option<String>,
}

/// A fixed id, or an id containing exactly one `<slug>` placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdPattern(String);

impl IdPattern {
    /// The rule as written in frontmatter.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Match a fixed id exactly, or substitute a lowercase kebab-case slug.
    #[must_use]
    pub fn matches(&self, id: &str) -> bool {
        match self.0.split_once("<slug>") {
            Some((prefix, suffix)) => id
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .is_some_and(is_slug),
            None => id == self.0,
        }
    }
}

/// Rules parsed from one template file, plus its Markdown skeleton.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// Template name, used by page frontmatter.
    pub template: String,
    /// Required knowledge layer (`type` in frontmatter).
    pub kind: TemplateKind,
    /// Fixed page id or slug pattern.
    pub id: IdPattern,
    /// Suggested page title, possibly containing a placeholder.
    pub title: String,
    /// Whether the Library may contain only one page using this template.
    pub one_per_library: bool,
    /// Automatic attachment policy.
    pub attach: AttachPolicy,
    /// Default sensitivity for new pages.
    pub sensitivity: Sensitivity,
    /// Suggested level-two headings, in order.
    pub sections: Vec<String>,
    /// Headings that must have content on a page.
    pub required: Vec<String>,
    /// Who may carry the page's procedure out; `you` when the rule is absent.
    pub performable: Performable,
    /// Submission target for a performed runbook, when declared.
    pub submission: Option<Submission>,
    /// Markdown after the closing frontmatter delimiter, preserved verbatim.
    pub body: String,
}

impl Template {
    /// Parse a flat YAML rules mapping followed by a Markdown skeleton.
    /// Scalars, inline lists and indented block lists are supported. Unknown,
    /// duplicate, missing and unsupported rules fail with the file and field.
    pub fn from_markdown(file: &str, text: &str) -> Result<Self> {
        let (frontmatter, body) = split_frontmatter(file, text)?;
        let rules = parse_rules(file, frontmatter)?;
        let string = |field| rule_string(file, &rules, field);
        let template = string("template")?;
        if !is_slug(&template) {
            return Err(invalid(
                file,
                "template",
                "expected a lowercase kebab-case name",
            ));
        }
        let kind = match string("type")?.as_str() {
            "domain" => TemplateKind::Domain,
            "runbook" => TemplateKind::Runbook,
            _ => return Err(invalid(file, "type", "expected domain or runbook")),
        };
        let pattern = string("id")?;
        let substituted = pattern.replacen("<slug>", "slug", 1);
        if !is_slug(&substituted) {
            return Err(invalid(
                file,
                "id",
                "expected a fixed id or one <slug> placeholder",
            ));
        }
        let one_per_library = rules
            .get("one_per_library")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid(file, "one_per_library", "expected true or false"))?;
        if one_per_library && pattern.contains("<slug>") {
            return Err(invalid(
                file,
                "id",
                "one-per-Library templates need a fixed id",
            ));
        }
        let attach = match string("attach")?.as_str() {
            "always" => AttachPolicy::Always,
            "participants" => AttachPolicy::Participants,
            "tagged" => AttachPolicy::Tagged,
            "matched" => AttachPolicy::Matched,
            "never" => AttachPolicy::Never,
            _ => {
                return Err(invalid(
                    file,
                    "attach",
                    "expected always, participants, tagged, matched or never",
                ));
            }
        };
        let sensitivity = Sensitivity::parse(&string("sensitivity")?)
            .ok_or_else(|| invalid(file, "sensitivity", "expected normal or private"))?;
        let sections = rule_list(file, &rules, "sections")?;
        let required = rule_list(file, &rules, "required")?;
        if required.iter().any(|section| !sections.contains(section)) {
            return Err(invalid(
                file,
                "required",
                "required headings must appear in sections",
            ));
        }
        let performable = match rules.get("performable") {
            None => Performable::You,
            Some(value) => match value.as_str() {
                Some("agent") => Performable::Agent,
                Some("you") => Performable::You,
                _ => {
                    return Err(invalid(file, "performable", "expected agent or you"));
                }
            },
        };
        let submission = match rules.get("submission") {
            None => None,
            Some(value) => {
                let block = value
                    .as_object()
                    .ok_or_else(|| invalid(file, "submission", "expected a nested mapping"))?;
                let sub = |field: &str| -> Result<Option<String>> {
                    match block.get(field) {
                        None => Ok(None),
                        Some(v) => v
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| Some(s.to_owned()))
                            .ok_or_else(|| invalid(file, field, "expected a nonempty string rule")),
                    }
                };
                let system = sub("system")?
                    .ok_or_else(|| invalid(file, "system", "expected a nonempty string rule"))?;
                let via_raw =
                    sub("via")?.ok_or_else(|| invalid(file, "via", "expected draft or gateway"))?;
                let via = match via_raw.as_str() {
                    "draft" => SubmissionVia::Draft,
                    "gateway" => SubmissionVia::Gateway,
                    _ => return Err(invalid(file, "via", "expected draft or gateway")),
                };
                let connection = sub("connection")?;
                if via == SubmissionVia::Gateway && connection.is_none() {
                    return Err(invalid(
                        file,
                        "connection",
                        "via gateway requires a connection",
                    ));
                }
                Some(Submission {
                    system,
                    via,
                    connection,
                })
            }
        };
        Ok(Self {
            template,
            kind,
            id: IdPattern(pattern),
            title: string("title")?,
            one_per_library,
            attach,
            sensitivity,
            sections,
            required,
            performable,
            submission,
            body: body.into(),
        })
    }

    /// Validate the resolved page id, frontmatter type and Markdown body.
    /// Template lookup and Library uniqueness checks belong to the caller.
    pub fn validate_page(&self, id: &str, kind: DocumentKind, body: &str) -> Result<()> {
        if !self.id.matches(id) {
            return Err(Error::PageIdMismatch {
                id: id.into(),
                template: self.template.clone(),
                pattern: self.id.as_str().into(),
            });
        }
        if kind != self.kind.document_kind() {
            return Err(Error::TemplateTypeMismatch {
                id: id.into(),
                template: self.template.clone(),
                expected: self.kind.as_str(),
                actual: kind.as_str(),
            });
        }
        if self.template == "preferences" {
            crate::domain::knowledge::preferences::parse(id, body)?;
        }
        let populated = populated_sections(body);
        for section in &self.required {
            if !populated.iter().any(|heading| heading == section) {
                return Err(Error::MissingSection {
                    id: id.into(),
                    template: self.template.clone(),
                    section: section.clone(),
                });
            }
        }
        Ok(())
    }
}
