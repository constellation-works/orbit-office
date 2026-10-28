//! Knowledge pages (About and How-to): the frontmatter model, the base
//! rules `office.knowledge-write` enforces, citation and anchor resolution,
//! template checks and the model view (specs/library.md#knowledge-pages,
//! specs/knowledge-templates.md, specs/library-bootstrap.md#page-rules).

use std::path::Path;

use office_common::Sensitivity;
use serde::Serialize;
use serde_json::Value;

use super::frontmatter;
use super::{DocumentKind, DocumentRecord};
use crate::domain::source::parse_clock_seconds;
use crate::domain::template::{Template, TemplateFile, frontmatter_scalar};
use crate::{Error, Result};

/// The word cap on a page body (library-bootstrap.md, "Short pages").
pub const MAX_PAGE_WORDS: usize = 1_200;

/// The tag that marks a page, or (as a trailing `[policy]` on its heading)
/// a section, as a policy summary.
pub const POLICY_TAG: &str = "policy";

const FIELDS: &[&str] = &[
    "id",
    "type",
    "template",
    "title",
    "aliases",
    "summary",
    "tags",
    "cites",
    "refs",
    "related",
    "updated_at",
    "last_verified",
    "status",
    "sensitivity",
    "shareable",
    "shared_by",
    "contributors",
    "as_of",
    "performable",
    "submission",
];

/// A page's lifecycle (`status`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    /// In use.
    #[default]
    Current,
    /// Due for re-verification.
    Stale,
    /// Kept for history only.
    Retired,
}

/// Who carries out a runbook (`performable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Performable {
    /// The agent does every step up to submission.
    Agent,
    /// The user does it (the default when absent).
    You,
}

/// How a performable runbook's request is submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionVia {
    /// A `draft` artifact the user submits.
    Draft,
    /// A `gateway.http-draft` the user approves.
    Gateway,
}

/// A runbook's `submission` mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Submission {
    /// Where the request goes, for example `ServiceNow`.
    pub system: String,
    /// Draft or gateway.
    pub via: SubmissionVia,
    /// The gateway connection; required when `via` is `gateway`.
    pub connection: Option<String>,
}

/// Which kind of pointer a ref is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefTarget {
    /// A file path.
    Path,
    /// A link.
    Url,
    /// An email address or phone number. Never reaches a model.
    Contact,
    /// A key in another system, `system:key`.
    Account,
    /// An artifact id.
    Artifact,
}

impl RefTarget {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "path" => Some(Self::Path),
            "url" => Some(Self::Url),
            "contact" => Some(Self::Contact),
            "account" => Some(Self::Account),
            "artifact" => Some(Self::Artifact),
            _ => None,
        }
    }
}

/// One pointer in a domain page's `refs`. Pointers are never resolved or
/// followed, and never carry credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRef {
    /// What the pointer is.
    pub label: String,
    /// The target kind.
    pub target: RefTarget,
    /// The target value.
    pub value: String,
    /// Whether this is the official source of a policy summary.
    pub authoritative: bool,
    /// The date the pointed-to material is current as of, `YYYY-MM-DD`.
    pub as_of: Option<String>,
}

/// One citation, from frontmatter `cites` or inline in the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Citation {
    /// The citation as written.
    pub text: String,
    /// The cited document id.
    pub id: String,
    /// The kind of document the id prefix names.
    pub kind: DocumentKind,
    /// The anchor after `#`, if any.
    pub anchor: Option<String>,
}

/// A parsed knowledge page: frontmatter fields plus the verbatim body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// `dom-…` or `rb-…`.
    pub id: String,
    /// `Domain` or `Runbook` (frontmatter `type`, else the id prefix).
    pub kind: DocumentKind,
    /// The template the page follows, if any.
    pub template: Option<String>,
    /// Title.
    pub title: String,
    /// Names the subject goes by in sources.
    pub aliases: Vec<String>,
    /// One-line summary.
    pub summary: String,
    /// Tags, for example `client:acme`.
    pub tags: Vec<String>,
    /// Frontmatter citations, as written.
    pub cites: Vec<String>,
    /// Pointers (domain pages only).
    pub refs: Vec<PageRef>,
    /// Related page ids.
    pub related: Vec<String>,
    /// Last update time, as written.
    pub updated_at: Option<String>,
    /// Last verification date, `YYYY-MM-DD`.
    pub last_verified: Option<String>,
    /// Lifecycle.
    pub status: PageStatus,
    /// Sensitivity (`normal` when absent).
    pub sensitivity: Sensitivity,
    /// Explicit approval to reach the shared Library.
    pub shareable: bool,
    /// The date the page's policy content is current as of, `YYYY-MM-DD`.
    pub as_of: Option<String>,
    /// Who performs a runbook.
    pub performable: Option<Performable>,
    /// How a performable runbook is submitted.
    pub submission: Option<Submission>,
    /// Markdown after the frontmatter, verbatim.
    pub body: String,
}

fn invalid(field: &str, reason: &'static str) -> Error {
    Error::InvalidFrontmatter {
        field: field.into(),
        reason,
    }
}

impl Page {
    /// Parse and validate a page's frontmatter. Unknown fields, malformed
    /// values and refs that are not single plain pointers are refused with
    /// `invalid_frontmatter`, naming the field but never echoing its value.
    pub fn from_markdown(markdown: &str) -> Result<Self> {
        let (frontmatter, body) = frontmatter::split(markdown)?;
        let fields = frontmatter::parse(frontmatter)?;
        if let Some((key, _)) = fields
            .iter()
            .find(|(key, _)| !FIELDS.contains(&key.as_str()))
        {
            return Err(invalid(key, "unknown field"));
        }
        let get = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                .filter(|value| !value.is_null())
        };

        if let Some(member) = text(get("shared_by"), "shared_by")?
            && (member.len() > 32 || !is_slug(&member))
        {
            return Err(invalid("shared_by", "expected a member name"));
        }
        if list(get("contributors"), "contributors")?
            .iter()
            .any(|member| member.len() > 32 || !is_slug(member))
        {
            return Err(invalid("contributors", "expected member names"));
        }
        let id = text(get("id"), "id")?.ok_or_else(|| invalid("id", "required"))?;
        let prefix_kind = page_kind_for_id(&id)
            .ok_or_else(|| invalid("id", "expected dom-<slug> or rb-<slug>"))?;
        let kind = match text(get("type"), "type")?.as_deref() {
            None => prefix_kind,
            Some("domain") => DocumentKind::Domain,
            Some("runbook") => DocumentKind::Runbook,
            Some(_) => return Err(invalid("type", "expected domain or runbook")),
        };
        let template = text(get("template"), "template")?;
        if template.as_deref().is_some_and(|name| !is_slug(name)) {
            return Err(invalid("template", "expected a lowercase kebab-case name"));
        }
        let title = text(get("title"), "title")?
            .filter(|title| !title.trim().is_empty())
            .ok_or_else(|| invalid("title", "required"))?;
        let cites = list(get("cites"), "cites")?;
        let related = list(get("related"), "related")?;
        if related.iter().any(|id| page_kind_for_id(id).is_none()) {
            return Err(invalid("related", "expected page ids (dom-… or rb-…)"));
        }
        let status = match text(get("status"), "status")?.as_deref() {
            None | Some("current") => PageStatus::Current,
            Some("stale") => PageStatus::Stale,
            Some("retired") => PageStatus::Retired,
            Some(_) => return Err(invalid("status", "expected current, stale or retired")),
        };
        let sensitivity = match text(get("sensitivity"), "sensitivity")? {
            None => Sensitivity::Normal,
            Some(token) => Sensitivity::parse(&token)
                .ok_or_else(|| invalid("sensitivity", "expected normal or private"))?,
        };
        let last_verified = date(get("last_verified"), "last_verified")?;
        let as_of = date(get("as_of"), "as_of")?;
        let performable = match text(get("performable"), "performable")?.as_deref() {
            None => None,
            Some("agent") => Some(Performable::Agent),
            Some("you") => Some(Performable::You),
            Some(_) => return Err(invalid("performable", "expected agent or you")),
        };
        let submission = get("submission").map(submission).transpose()?;
        let refs = match get("refs") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.iter().map(page_ref).collect::<Result<_>>()?,
            Some(_) => return Err(invalid("refs", "expected a list of pointers")),
        };

        Ok(Self {
            id,
            kind,
            template,
            title,
            aliases: list(get("aliases"), "aliases")?,
            summary: text(get("summary"), "summary")?.unwrap_or_default(),
            tags: list(get("tags"), "tags")?,
            cites,
            refs,
            related,
            updated_at: text(get("updated_at"), "updated_at")?,
            last_verified,
            status,
            sensitivity,
            shareable: crate::domain::sharing::shareable(markdown)?,
            as_of,
            performable,
            submission,
            body: body.to_string(),
        })
    }

    /// The page's Library-relative path: `knowledge/domain/<id>.md` or
    /// `knowledge/runbooks/<id>.md`.
    #[must_use]
    pub fn path(&self) -> String {
        match self.kind {
            DocumentKind::Runbook => format!("knowledge/runbooks/{}.md", self.id),
            DocumentKind::Domain | DocumentKind::Source | DocumentKind::Artifact => {
                format!("knowledge/domain/{}.md", self.id)
            }
        }
    }

    /// The base rules every page meets, with or without a template: the id
    /// prefix matches the type, runbook-only and domain-only fields stay on
    /// their layer, no contact details in the body, the word cap, and a
    /// dated authoritative source for policy summaries.
    pub fn check_base_rules(&self) -> Result<()> {
        if page_kind_for_id(&self.id) != Some(self.kind) {
            return Err(invalid(
                "type",
                "dom- ids are domain pages, rb- ids runbooks",
            ));
        }
        if self.kind == DocumentKind::Domain {
            if self.performable.is_some() {
                return Err(invalid("performable", "only runbooks are performable"));
            }
            if self.submission.is_some() {
                return Err(invalid("submission", "only runbooks have a submission"));
            }
        }
        if self.kind == DocumentKind::Runbook && !self.refs.is_empty() {
            return Err(invalid("refs", "pointers belong on domain pages"));
        }
        if let Some((_, kind)) = find_contacts(&self.body).first() {
            return Err(Error::ContactInBody {
                id: self.id.clone(),
                kind,
            });
        }
        let words = self.body.split_whitespace().count();
        if words > MAX_PAGE_WORDS {
            return Err(Error::PageTooLong {
                id: self.id.clone(),
                words,
                limit: MAX_PAGE_WORDS,
            });
        }
        if self.is_policy() {
            let sourced = self
                .refs
                .iter()
                .any(|r| r.authoritative && (r.as_of.is_some() || self.as_of.is_some()));
            if !sourced {
                return Err(Error::PolicyWithoutSource {
                    id: self.id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Whether the page, or one of its sections, is tagged `policy`.
    #[must_use]
    pub fn is_policy(&self) -> bool {
        self.tags.iter().any(|tag| tag == POLICY_TAG)
            || headings(&self.body).any(|(level, title)| {
                level >= 2
                    && title
                        .strip_suffix(']')
                        .and_then(|rest| rest.strip_suffix(POLICY_TAG))
                        .is_some_and(|rest| rest.ends_with('['))
            })
    }

    /// Every citation the page makes: frontmatter `cites`, then inline
    /// `[id#anchor]` citations in the body, without duplicates.
    pub fn citations(&self) -> Result<Vec<Citation>> {
        let mut citations: Vec<Citation> = Vec::new();
        let written = self
            .cites
            .iter()
            .cloned()
            .chain(inline_citations(&self.body));
        for text in written {
            if citations.iter().any(|c| c.text == text) {
                continue;
            }
            let citation = parse_citation(&text).ok_or_else(|| Error::UnresolvedCite {
                id: self.id.clone(),
                cite: text.clone(),
                reason: "expected <id> or <id>#<anchor> naming a src-, dom-, rb- or art- document",
            })?;
            citations.push(citation);
        }
        Ok(citations)
    }

    /// Resolve every citation through `lookup` (an id to its indexed
    /// document): the document exists and is the kind its prefix names, a
    /// source cite names an anchor present in that source, and no other
    /// document is cited with an anchor.
    pub fn resolve_citations(
        &self,
        mut lookup: impl FnMut(&str) -> Result<Option<DocumentRecord>>,
    ) -> Result<()> {
        for citation in self.citations()? {
            let refuse = |reason| Error::UnresolvedCite {
                id: self.id.clone(),
                cite: citation.text.clone(),
                reason,
            };
            if citation.id == self.id {
                return Err(refuse("a page cannot cite itself"));
            }
            let Some(record) = lookup(&citation.id)? else {
                return Err(refuse(match citation.kind {
                    DocumentKind::Source => "no source has this id",
                    DocumentKind::Domain | DocumentKind::Runbook | DocumentKind::Artifact => {
                        "no document has this id"
                    }
                }));
            };
            if record.id == self.id {
                return Err(refuse("a page cannot cite itself"));
            }
            if record.kind != citation.kind {
                return Err(refuse("the id names a different kind of document"));
            }
            match (citation.kind, citation.anchor.as_deref()) {
                (DocumentKind::Source, None) => {
                    return Err(refuse("a source cite needs an anchor"));
                }
                (DocumentKind::Source, Some(anchor)) => {
                    resolve_anchor(&record.body, anchor).map_err(refuse)?;
                }
                (_, Some(_)) => return Err(refuse("anchors are defined only for sources")),
                (_, None) => {}
            }
        }
        Ok(())
    }

    /// Check the page against the template it names (knowledge-templates.md,
    /// "The template field on pages"). `templates` are the Library's template
    /// files in path order (the first valid file declaring a name is current);
    /// `pages` are the Library's other knowledge pages. A page without a
    /// template passes.
    pub fn check_template(
        &self,
        templates: &[TemplateFile<'_>],
        pages: &[PageFile<'_>],
    ) -> Result<()> {
        let Some(name) = self.template.as_deref() else {
            return Ok(());
        };
        let mut broken = None;
        let mut found = None;
        for file in templates {
            let parsed = file
                .markdown
                .ok_or_else(|| Error::TemplateInvalid {
                    file: file.path.into(),
                    field: "file".into(),
                    reason: "the template file could not be read",
                })
                .and_then(|markdown| Template::from_markdown(file.path, markdown));
            match parsed {
                Ok(template) if template.template == name => {
                    found = Some(template);
                    break;
                }
                Ok(_) => {}
                Err(error) => {
                    let stem = Path::new(file.path).file_stem().and_then(|s| s.to_str());
                    if stem == Some(name) && broken.is_none() {
                        broken = Some(error);
                    }
                }
            }
        }
        let Some(template) = found else {
            return Err(broken.unwrap_or_else(|| Error::UnknownTemplate {
                id: self.id.clone(),
                template: name.into(),
            }));
        };
        if template.one_per_library && template.id.matches(&self.id) {
            for page in pages {
                let record = DocumentRecord::from_markdown(Path::new(page.path), page.markdown);
                if record.id != self.id
                    && frontmatter_scalar(page.markdown, "template").as_deref() == Some(name)
                {
                    return Err(Error::TemplatePageExists {
                        id: self.id.clone(),
                        template: name.into(),
                        existing: record.id,
                    });
                }
            }
        }
        template.validate_page(&self.id, self.kind, &self.body)
    }

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

/// One knowledge page file, as listed from the Library.
#[derive(Clone, Copy, Debug)]
pub struct PageFile<'a> {
    /// Library-relative path.
    pub path: &'a str,
    /// Full Markdown, including frontmatter.
    pub markdown: &'a str,
}

/// The knowledge layer an id's prefix names, if it is a page id.
#[must_use]
pub fn page_kind_for_id(id: &str) -> Option<DocumentKind> {
    if id.strip_prefix("dom-").is_some_and(is_slug) {
        Some(DocumentKind::Domain)
    } else if id.strip_prefix("rb-").is_some_and(is_slug) {
        Some(DocumentKind::Runbook)
    } else {
        None
    }
}

/// Parse `<id>` or `<id>#<anchor>` where the id is `src-`, `dom-`, `rb-` or
/// `art-` followed by a lowercase kebab-case slug.
#[must_use]
pub fn parse_citation(text: &str) -> Option<Citation> {
    let (id, anchor) = match text.split_once('#') {
        Some((id, anchor)) => (id, Some(anchor)),
        None => (text, None),
    };
    if anchor.is_some_and(|a| a.is_empty() || a.contains(char::is_whitespace)) {
        return None;
    }
    let local = if let Some((member, local)) = id.split_once('/') {
        if member.is_empty() || member.len() > 32 || !is_slug(member) {
            return None;
        }
        local
    } else {
        id
    };
    let (prefix, slug) = local.split_once('-')?;
    let kind = match prefix {
        "src" => DocumentKind::Source,
        "dom" => DocumentKind::Domain,
        "rb" => DocumentKind::Runbook,
        "art" => DocumentKind::Artifact,
        _ => return None,
    };
    is_slug(slug).then(|| Citation {
        text: text.into(),
        id: id.into(),
        kind,
        anchor: anchor.map(str::to_owned),
    })
}

/// Check that `anchor` names a place in a source body (specs/library.md,
/// Sources): `t=hh:mm:ss` within the speaker turns (from the first turn's
/// timestamp to the last's), `L<n>` or `L<n>-<m>` within the body's lines,
/// and `slide=<n>` or `page=<n>` naming a `Slide <n>` or `Page <n>` heading.
/// Frame and chapter anchors live beside the source and are refused until
/// those files are readable here. Returns the refusal reason.
pub fn resolve_anchor(body: &str, anchor: &str) -> std::result::Result<(), &'static str> {
    if let Some(time) = anchor.strip_prefix("t=") {
        let at = parse_clock_seconds(time).ok_or("expected t=hh:mm:ss within u64 seconds")?;
        let turns: Vec<u64> = body
            .lines()
            .filter_map(|line| {
                let rest = line.trim_start().strip_prefix('[')?;
                let (stamp, _) = rest.split_once(']')?;
                parse_clock_seconds(stamp)
            })
            .collect();
        let (Some(first), Some(last)) = (turns.iter().min(), turns.iter().max()) else {
            return Err("the source has no timestamped turns");
        };
        return if (*first..=*last).contains(&at) {
            Ok(())
        } else {
            Err("the time is outside the source's turns")
        };
    }
    if let Some(range) = anchor.strip_prefix('L') {
        let (start, end) = match range.split_once('-') {
            Some((start, end)) => (start, end),
            None => (range, range),
        };
        let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) else {
            return Err("expected L<n> or L<n>-<m>");
        };
        let lines = body.lines().count();
        return if start >= 1 && start <= end && end <= lines {
            Ok(())
        } else {
            Err("the line range is outside the source")
        };
    }
    if anchor.starts_with("slide=") || anchor.starts_with("page=") {
        let parsed = crate::domain::source::parse_anchor(anchor)
            .map_err(|_| "expected a positive slide or page number")?;
        return crate::domain::source::anchor_span(body, parsed, anchor)
            .map(|_| ())
            .map_err(|_| "no such slide or page heading in the source");
    }
    if anchor.starts_with("frame=") || anchor.starts_with("chapter=") {
        return Err("frame and chapter anchors are not resolvable yet");
    }
    Err("unknown anchor form")
}

/// `[id#anchor]` citations in a body: bracketed text whose every `;`- or
/// `,`-separated item parses as a citation.
pub(crate) fn inline_citations(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find([']', '[', '\n']) else {
            break;
        };
        if after.as_bytes()[close] == b']' {
            let inner = &after[..close];
            let items: Vec<&str> = inner.split([';', ',']).map(str::trim).collect();
            if !items.is_empty() && items.iter().all(|item| parse_citation(item).is_some()) {
                found.extend(items.into_iter().map(str::to_owned));
            }
        }
        rest = &after[close..];
        if rest.starts_with(']') {
            rest = &rest[1..];
        }
    }
    found
}

/// ATX headings outside fenced code: `(level, title)`.
fn headings(body: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut fence: Option<char> = None;
    body.lines().filter_map(move |line| {
        let trimmed = line.trim();
        let first = trimmed.chars().next();
        if matches!(first, Some('`' | '~'))
            && trimmed.chars().take_while(|c| Some(*c) == first).count() >= 3
        {
            fence = match fence {
                Some(marker) if Some(marker) == first => None,
                None => first,
                other => other,
            };
            return None;
        }
        if fence.is_some() || line.starts_with("    ") || line.starts_with('\t') {
            return None;
        }
        let text = line.trim_start();
        let level = text.bytes().take_while(|c| *c == b'#').count();
        if !(1..=6).contains(&level) {
            return None;
        }
        let rest = &text[level..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        Some((level, rest.trim().trim_end_matches('#').trim_end()))
    })
}

/// Email addresses and phone numbers in `text`, as byte ranges and kinds.
fn find_contacts(text: &str) -> Vec<(std::ops::Range<usize>, &'static str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    // Emails: local@domain.tld.
    for (at, _) in text.match_indices('@') {
        let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
        let mut start = at;
        while start > 0 && local(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || b".-".contains(&bytes[end]))
        {
            end += 1;
        }
        while end > at + 1 && b".-".contains(&bytes[end - 1]) {
            end -= 1;
        }
        let domain = &text[at + 1..end];
        let tld_ok = domain.rsplit_once('.').is_some_and(|(host, tld)| {
            !host.is_empty() && tld.len() >= 2 && tld.bytes().all(|c| c.is_ascii_alphabetic())
        });
        if start < at && tld_ok {
            found.push((start..end, "email"));
        }
    }
    // Phone numbers: 10–15 digits in groups joined by single separators.
    let mut index = 0;
    while index < bytes.len() {
        let c = bytes[index];
        let boundary = index == 0 || {
            let before = bytes[index - 1];
            !(before.is_ascii_alphanumeric() || b"-_#=/:.@".contains(&before))
        };
        if !(boundary && (c.is_ascii_digit() || c == b'+' || c == b'(')) {
            index += 1;
            continue;
        }
        let start = index;
        let mut end = index;
        let mut digits = 0;
        let mut last_digit_end = index;
        while end < bytes.len() {
            let b = bytes[end];
            let next = bytes.get(end + 1).copied();
            let joins = match b {
                b'0'..=b'9' => {
                    digits += 1;
                    last_digit_end = end + 1;
                    true
                }
                // A closing parenthesis may be followed by a separator.
                b')' => next.is_some_and(|n| n.is_ascii_digit() || b" -.".contains(&n)),
                // Every other separator is single and leads to a digit group.
                b'(' | b'+' | b'-' | b'.' | b' ' => {
                    next.is_some_and(|n| n.is_ascii_digit() || n == b'(')
                }
                _ => false,
            };
            if !joins {
                break;
            }
            end += 1;
        }
        let run = &text[start..last_digit_end];
        let after_ok = bytes
            .get(last_digit_end)
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        if (10..=15).contains(&digits) && after_ok && !contains_iso_date(run) {
            found.push((start..last_digit_end, "phone"));
        }
        index = end.max(index + 1);
    }
    found.sort_by_key(|(range, _)| range.start);
    found
}

fn contains_iso_date(run: &str) -> bool {
    let bytes = run.as_bytes();
    bytes.windows(10).any(|w| {
        w[..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[7] == b'-'
            && w[8..].iter().all(u8::is_ascii_digit)
    })
}

/// Remove contact values before unstructured evidence reaches a model.
#[must_use]
pub fn withhold_contacts(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut at = 0;
    for (range, _) in find_contacts(body) {
        if range.start < at {
            continue;
        }
        out.push_str(&body[at..range.start]);
        out.push_str("[contact withheld]");
        at = range.end;
    }
    out.push_str(&body[at..]);
    out
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

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(invalid(field, "expected a string")),
    }
}

fn date(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    let value = text(value, field)?;
    if value.as_deref().is_some_and(|v| !is_date(v)) {
        return Err(invalid(field, "expected a YYYY-MM-DD date"));
    }
    Ok(value)
}

fn list(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(field, "expected a list of nonempty strings"))
            })
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(vec![text.clone()]),
        Some(_) => Err(invalid(field, "expected a list of strings")),
    }
}

fn submission(value: &Value) -> Result<Submission> {
    let Value::Object(map) = value else {
        return Err(invalid("submission", "expected a mapping"));
    };
    if map
        .keys()
        .any(|key| !["system", "via", "connection"].contains(&key.as_str()))
    {
        return Err(invalid(
            "submission",
            "unknown key; expected system, via, connection",
        ));
    }
    let string = |key: &str| text(map.get(key).filter(|v| !v.is_null()), "submission");
    let system = string("system")?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("submission", "system is required"))?;
    let via = match string("via")?.as_deref() {
        Some("draft") => SubmissionVia::Draft,
        Some("gateway") => SubmissionVia::Gateway,
        _ => return Err(invalid("submission", "via must be draft or gateway")),
    };
    let connection = string("connection")?;
    if via == SubmissionVia::Gateway && connection.is_none() {
        return Err(invalid(
            "submission",
            "a gateway submission names its connection",
        ));
    }
    Ok(Submission {
        system,
        via,
        connection,
    })
}

fn page_ref(value: &Value) -> Result<PageRef> {
    let Value::Object(map) = value else {
        return Err(invalid("refs", "expected { label, <target> } mappings"));
    };
    let mut target = None;
    for key in map.keys() {
        if let Some(kind) = RefTarget::parse(key) {
            if target.is_some() {
                return Err(invalid("refs", "a ref has exactly one target"));
            }
            target = Some((kind, key.as_str()));
        } else if !["label", "authoritative", "as_of"].contains(&key.as_str()) {
            return Err(invalid(
                "refs",
                "unknown key; refs are pointers (label, path, url, contact, account, artifact, authoritative, as_of)",
            ));
        }
    }
    let (kind, key) = target.ok_or_else(|| {
        invalid(
            "refs",
            "a ref needs one of path, url, contact, account, artifact",
        )
    })?;
    let label = text(map.get("label"), "refs")?
        .filter(|label| !label.trim().is_empty())
        .ok_or_else(|| invalid("refs", "a ref needs a label"))?;
    let value = text(map.get(key), "refs")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("refs", "a ref's target must be a nonempty string"))?;
    match kind {
        RefTarget::Account => {
            let valid = value
                .split_once(':')
                .is_some_and(|(system, key)| is_slug(system) && !key.trim().is_empty());
            if !valid {
                return Err(invalid("refs", "an account target is system:key"));
            }
        }
        RefTarget::Artifact => {
            if !value.strip_prefix("art-").is_some_and(is_slug) {
                return Err(invalid("refs", "an artifact target is an art- id"));
            }
        }
        RefTarget::Url => {
            let authority = value
                .split_once("://")
                .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default());
            match authority {
                None => return Err(invalid("refs", "a url target needs a scheme")),
                Some(authority) if authority.contains('@') => {
                    return Err(invalid("refs", "a url target never carries credentials"));
                }
                Some(_) => {}
            }
        }
        RefTarget::Path | RefTarget::Contact => {}
    }
    let authoritative = match map.get("authoritative") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(invalid("refs", "authoritative is true or false")),
    };
    Ok(PageRef {
        label,
        target: kind,
        value,
        authoritative,
        as_of: date(map.get("as_of").filter(|v| !v.is_null()), "refs")?,
    })
}
