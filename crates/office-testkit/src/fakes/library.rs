//! In-memory Library port with document, search and history behavior.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use office_core::domain::knowledge::{DocumentRecord, Revision, SearchFilter, SearchHit};
use office_core::domain::settings::Settings;
use office_core::ports::Library;
use office_core::{Error, Result};

/// A Library fake with explicit readiness and settings, and no host I/O.
#[derive(Default)]
pub struct FakeLibrary {
    documents: RefCell<BTreeMap<String, DocumentRecord>>,
    revisions: RefCell<BTreeMap<String, Vec<Revision>>>,
    /// Whether the fixture represents an initialized Library.
    pub initialized: bool,
    /// Settings returned by the fake (validated on every load).
    pub settings: Settings,
}

impl FakeLibrary {
    /// An initialized, empty fixture with policy defaults.
    #[must_use]
    pub fn initialized() -> Self {
        Self {
            initialized: true,
            ..Self::default()
        }
    }

    fn ready(&self) -> Result<()> {
        if !self.initialized {
            return Err(Error::Library {
                code: "library_not_initialized",
                message: "initialize the fake Library".into(),
            });
        }
        self.settings.validate()
    }
}

impl Library for FakeLibrary {
    fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        self.ready()?;
        self.documents
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned()
            .ok_or_else(|| Error::Library {
                code: "io_error",
                message: format!("document {} is absent", relative.display()),
            })
    }

    fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        self.ready()?;
        let record = DocumentRecord::from_markdown(relative, text);
        self.documents
            .borrow_mut()
            .insert(record.path.clone(), record.clone());
        let mut revisions = self.revisions.borrow_mut();
        let history = revisions.entry(record.path.clone()).or_default();
        history.insert(
            0,
            Revision {
                commit: format!("fake-{}", history.len() + 1),
                author: "orbit-office".into(),
                email: "office@orbit.local".into(),
                authored_at: "1970-01-01T00:00:00+00:00".into(),
                message: format!("Update {}", record.path),
            },
        );
        Ok(record)
    }

    fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
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
                    && filter.kind.is_none_or(|kind| doc.kind == kind)
                    && filter.tag.as_ref().is_none_or(|tag| doc.tags.contains(tag))
                    && filter
                        .series
                        .as_ref()
                        .is_none_or(|series| doc.series.as_ref() == Some(series))
            })
            .take(filter.limit.unwrap_or(20))
            .map(|doc| SearchHit {
                id: doc.id.clone(),
                kind: doc.kind,
                title: doc.title.clone(),
                path: doc.path.clone(),
                snippet: doc.body.chars().take(120).collect(),
            })
            .collect())
    }

    fn history(&self, relative: &Path) -> Result<Vec<Revision>> {
        self.ready()?;
        Ok(self
            .revisions
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned()
            .unwrap_or_default())
    }
    fn is_initialized(&self) -> Result<bool> {
        if self.initialized {
            self.settings.validate()?;
        }
        Ok(self.initialized)
    }
    fn settings(&self) -> Result<Settings> {
        self.ready()?;
        Ok(self.settings.clone())
    }
}
