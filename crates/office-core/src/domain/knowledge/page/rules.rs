//! Base rules, citation resolution and template checks for a page.

use std::path::Path;

use super::citation::{
    PageFile, inline_citations, page_kind_for_id, parse_citation, resolve_anchor,
};
use super::contacts::find_contacts;
use super::types::{Citation, MAX_PAGE_WORDS, POLICY_TAG, Page, SCRIBE_RULES_PAGE, invalid};
use crate::domain::knowledge::{DocumentKind, DocumentRecord};
use crate::domain::template::{Template, TemplateFile, frontmatter_scalar};
use crate::{Error, Result};

impl Page {
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
        if self.id.starts_with("dom-preferences-")
            && self.template.as_deref() != Some("preferences")
        {
            return Err(invalid(
                "template",
                "preference page ids require template: preferences",
            ));
        }
        if self.scribe_handoff.is_some() && self.id != SCRIBE_RULES_PAGE {
            return Err(invalid(
                "scribe",
                "scribe settings belong on dom-scribe-rules",
            ));
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
    /// `[id#anchor]` citations and preference matter `cites` in the body,
    /// without duplicates.
    pub fn citations(&self) -> Result<Vec<Citation>> {
        let mut citations: Vec<Citation> = Vec::new();
        let matter_cites = if self.template.as_deref() == Some("preferences") {
            crate::domain::knowledge::preferences::parse(&self.id, &self.body)?
                .into_iter()
                .flat_map(|p| p.cites)
                .collect()
        } else {
            Vec::new()
        };
        let written = self
            .cites
            .iter()
            .cloned()
            .chain(inline_citations(&self.body))
            .chain(matter_cites);
        for text in written {
            if citations.iter().any(|c| c.text == text) {
                continue;
            }
            let citation = parse_citation(&text).ok_or_else(|| Error::UnresolvedCite {
                id: self.id.clone(),
                cite: text.clone(),
                reason: "expected <id> or <id>#<anchor> naming a src-, SCR-, dec-, dom-, rb- or art- document",
            })?;
            citations.push(citation);
        }
        Ok(citations)
    }

    /// Resolve every citation through `lookup` (an id to its indexed
    /// document): the document exists and is the kind its prefix names, a
    /// source cite names an anchor present in that source, and no other
    /// document is cited with an anchor. A decision ledger entry (a `dec-`
    /// id, or a migrated note's former `src-` id) may be cited bare; an
    /// anchor on it names a line of its verbatim quote.
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
                (DocumentKind::Source, None)
                    if crate::domain::source::decisions::is_entry_record(&record) => {}
                (DocumentKind::Source, None) => {
                    return Err(refuse("a source cite needs an anchor"));
                }
                (DocumentKind::Source, Some(anchor)) => {
                    if anchor.starts_with('T')
                        && !citation
                            .id
                            .rsplit('/')
                            .next()
                            .is_some_and(|id| id.starts_with("SCR-"))
                    {
                        return Err(refuse("tag anchors are defined only for SCR sources"));
                    }
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
