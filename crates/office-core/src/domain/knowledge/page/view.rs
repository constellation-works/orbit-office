//! The model view of a page.

use office_common::Sensitivity;
use serde::Serialize;

use super::contacts::withhold_contacts;
use super::types::{Page, PageStatus, Performable, RefTarget, Submission, SubmissionVia};
use crate::domain::knowledge::DocumentKind;

impl Page {
    /// The page as a model may see it: every `contact` ref keeps its label
    /// and loses its value, and any email address or phone number left in a
    /// hand-edited body is withheld. Every model input and task attachment
    /// carries this view (knowledge-templates.md#contact-details).
    #[must_use]
    pub fn model_view(&self) -> PageView {
        PageView {
            id: self.id.clone(),
            kind: self.kind,
            path: self.path(),
            template: self.template.clone(),
            title: self.title.clone(),
            aliases: self.aliases.clone(),
            summary: self.summary.clone(),
            tags: self.tags.clone(),
            cites: self.cites.clone(),
            refs: self
                .refs
                .iter()
                .map(|r| RefView {
                    label: r.label.clone(),
                    target: r.target,
                    value: (r.target != RefTarget::Contact).then(|| r.value.clone()),
                    authoritative: r.authoritative,
                    as_of: r.as_of.clone(),
                })
                .collect(),
            related: self.related.clone(),
            updated_at: self.updated_at.clone(),
            last_verified: self.last_verified.clone(),
            status: self.status,
            sensitivity: self.sensitivity,
            shareable: self.shareable,
            as_of: self.as_of.clone(),
            performable: self.performable,
            submission: self.submission.clone(),
            body: withhold_contacts(&self.body),
        }
    }
}

/// A ref in the model view: a contact's value is `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefView {
    /// What the pointer is.
    pub label: String,
    /// The target kind.
    pub target: RefTarget,
    /// The target value; always `None` for a contact.
    pub value: Option<String>,
    /// Whether this is a policy's official source.
    pub authoritative: bool,
    /// The date the pointed-to material is current as of.
    pub as_of: Option<String>,
}

/// The model view of a page ([`Page::model_view`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PageView {
    /// Page id.
    pub id: String,
    /// `domain` or `runbook`.
    #[serde(rename = "type")]
    pub kind: DocumentKind,
    /// Library-relative path.
    pub path: String,
    /// Template name.
    pub template: Option<String>,
    /// Title.
    pub title: String,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Summary.
    pub summary: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Frontmatter citations.
    pub cites: Vec<String>,
    /// Pointers, contact values withheld.
    pub refs: Vec<RefView>,
    /// Related page ids.
    pub related: Vec<String>,
    /// Last update time.
    pub updated_at: Option<String>,
    /// Last verification date.
    pub last_verified: Option<String>,
    /// Lifecycle.
    pub status: PageStatus,
    /// Sensitivity.
    pub sensitivity: Sensitivity,
    /// Sharing approval retained in model attachments when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shareable: bool,
    /// Policy currency date.
    pub as_of: Option<String>,
    /// Who performs a runbook.
    pub performable: Option<Performable>,
    /// How a performable runbook is submitted.
    pub submission: Option<Submission>,
    /// The body, with any contact detail withheld.
    pub body: String,
}

impl PageView {
    /// The view as page Markdown, for task attachments: frontmatter whose
    /// strings are double-quoted (valid YAML), then the body.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let quote = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
        let quoted_list = |items: &[String]| {
            items
                .iter()
                .map(|i| quote(i))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut out = String::from("---\n");
        let mut line = |key: &str, value: String| {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        };
        line("id", quote(&self.id));
        line("type", quote(self.kind.as_str()));
        if let Some(template) = &self.template {
            line("template", quote(template));
        }
        line("title", quote(&self.title));
        line("aliases", format!("[{}]", quoted_list(&self.aliases)));
        line("summary", quote(&self.summary));
        line("tags", format!("[{}]", quoted_list(&self.tags)));
        line("cites", format!("[{}]", quoted_list(&self.cites)));
        let refs: Vec<String> = self
            .refs
            .iter()
            .map(|r| {
                let target = match r.target {
                    RefTarget::Path => "path",
                    RefTarget::Url => "url",
                    RefTarget::Contact => "contact",
                    RefTarget::Account => "account",
                    RefTarget::Artifact => "artifact",
                };
                let value = r.value.as_deref().map_or_else(|| "null".into(), quote);
                let mut entry = format!("{{ label: {}, {target}: {value}", quote(&r.label));
                if r.authoritative {
                    entry.push_str(", authoritative: true");
                }
                if let Some(as_of) = &r.as_of {
                    entry.push_str(&format!(", as_of: {}", quote(as_of)));
                }
                entry.push_str(" }");
                entry
            })
            .collect();
        line("refs", format!("[{}]", refs.join(", ")));
        line("related", format!("[{}]", quoted_list(&self.related)));
        let status = match self.status {
            PageStatus::Current => "current",
            PageStatus::Stale => "stale",
            PageStatus::Retired => "retired",
        };
        line("status", quote(status));
        line("sensitivity", quote(self.sensitivity.as_str()));
        if self.shareable {
            line("shareable", "true".into());
        }
        for (key, value) in [
            ("updated_at", &self.updated_at),
            ("last_verified", &self.last_verified),
            ("as_of", &self.as_of),
        ] {
            if let Some(value) = value {
                line(key, quote(value));
            }
        }
        if let Some(performable) = self.performable {
            let token = match performable {
                Performable::Agent => "agent",
                Performable::You => "you",
            };
            line("performable", quote(token));
        }
        if let Some(submission) = &self.submission {
            let via = match submission.via {
                SubmissionVia::Draft => "draft",
                SubmissionVia::Gateway => "gateway",
            };
            let mut entry = format!(
                "{{ system: {}, via: {}",
                quote(&submission.system),
                quote(via)
            );
            if let Some(connection) = &submission.connection {
                entry.push_str(&format!(", connection: {}", quote(connection)));
            }
            entry.push_str(" }");
            line("submission", entry);
        }
        out.push_str("---\n");
        out.push_str(&self.body);
        out
    }
}
