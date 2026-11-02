use super::*;

impl FakeLibrary {
    pub(super) fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        self.ready()?;
        Ok(self
            .documents
            .borrow()
            .values()
            .filter(|doc| {
                let haystack = format!(
                    "{} {} {} {}",
                    doc.title,
                    doc.summary,
                    doc.body,
                    doc.tags.join(" ")
                )
                .to_lowercase();
                query
                    .split_whitespace()
                    .all(|term| haystack.contains(&term.to_lowercase()))
                    && filter.layer.is_none_or(|layer| doc.layer == layer)
                    && filter.kind.is_none_or(|kind| doc.kind == kind)
                    && filter.tag.as_ref().is_none_or(|tag| doc.tags.contains(tag))
                    && filter
                        .series
                        .as_ref()
                        .is_none_or(|series| doc.series.as_ref() == Some(series))
                    && filter
                        .template
                        .as_ref()
                        .is_none_or(|template| doc.template.as_ref() == Some(template))
                    && filter
                        .since
                        .as_ref()
                        .is_none_or(|since| doc.date.as_ref().is_some_and(|date| date >= since))
                    && filter
                        .until
                        .as_ref()
                        .is_none_or(|until| doc.date.as_ref().is_some_and(|date| date <= until))
            })
            .take(filter.limit.unwrap_or(20))
            .map(|doc| SearchHit {
                layer: doc.layer,
                shared_by: doc.shared_by.clone(),
                id: doc.id.clone(),
                kind: doc.kind,
                title: doc.title.clone(),
                path: doc.path.clone(),
                snippet: doc.body.chars().take(120).collect(),
                sensitivity: doc.sensitivity.clone(),
            })
            .collect())
    }

    pub(super) fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        use office_core::domain::knowledge::{LibraryLayer, page_kind_for_id};
        self.ready()?;
        let docs = self.documents.borrow();
        let own = |id: &str| {
            docs.values()
                .find(|doc| doc.id == id && doc.layer == LibraryLayer::Own)
                .cloned()
        };
        let shared = |id: &str| {
            docs.values()
                .find(|doc| doc.id == id && doc.layer == LibraryLayer::Shared)
                .cloned()
        };
        if let Some((member, local)) = id.split_once('/') {
            if self.settings()?.shared.is_some_and(|s| s.member == member) {
                return Ok(own(local));
            }
            return Ok(shared(if page_kind_for_id(local).is_some() {
                local
            } else {
                id
            })
            .filter(|doc| doc.shared_by.as_deref() == Some(member)));
        }
        Ok(own(id).or_else(|| shared(id)))
    }

    pub(super) fn backlinks(&self, id: &str) -> Result<Vec<Backlink>> {
        self.ready()?;
        let anchored = format!("{id}#");
        let mut links: Vec<Backlink> = self
            .documents
            .borrow()
            .values()
            .filter(|doc| doc.id != id)
            .filter_map(|doc| {
                let mut via = Vec::new();
                if doc
                    .cites
                    .iter()
                    .any(|cite| cite == id || cite.starts_with(&anchored))
                {
                    via.push(LinkKind::Cites);
                }
                if doc.related.iter().any(|related| related == id) {
                    via.push(LinkKind::Related);
                }
                (!via.is_empty()).then(|| Backlink {
                    id: doc.id.clone(),
                    kind: doc.kind,
                    title: doc.title.clone(),
                    path: doc.path.clone(),
                    via,
                })
            })
            .collect();
        links.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(links)
    }

    pub(super) fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| {
                office_core::domain::knowledge::local_document_path(path).starts_with("knowledge/")
            })
            .map(|(path, markdown)| LibraryText {
                path: path.clone(),
                markdown: Some(markdown.clone()),
            })
            .collect())
    }

    pub(super) fn source_documents(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| path.starts_with("sources/") && path.ends_with("/source.md"))
            .map(|(path, markdown)| LibraryText {
                path: path.clone(),
                markdown: Some(markdown.clone()),
            })
            .collect())
    }

    pub(super) fn templates(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self.templates.clone())
    }

    pub(super) fn history(&self, relative: &Path) -> Result<Vec<Revision>> {
        self.ready()?;
        Ok(self
            .revisions
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned()
            .unwrap_or_default())
    }

    pub(super) fn read_text(&self, relative: &Path) -> Result<String> {
        self.ready()?;
        self.texts
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned()
            .ok_or_else(|| Error::Library {
                code: "io_error",
                message: format!("document {} is absent", relative.display()),
            })
    }

    pub(super) fn head_commit(&self) -> Result<Option<String>> {
        self.ready()?;
        let commits = *self.commits.borrow();
        Ok((commits > 0).then(|| format!("fake-head-{commits}")))
    }
}
