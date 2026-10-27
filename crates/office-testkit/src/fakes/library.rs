//! In-memory Library port with document, search and history behavior.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use office_core::domain::knowledge::{
    Backlink, DocumentKind, DocumentRecord, LinkKind, Revision, SearchFilter, SearchHit,
};
use office_core::domain::settings::Settings;
use office_core::domain::template::BUNDLED;
use office_core::ports::{InitOutcome, Library, LibraryHealth, LibraryText, not_initialized_error};
use office_core::{Error, Result};

/// A Library fake with explicit readiness and settings, and no host I/O.
#[derive(Default)]
pub struct FakeLibrary {
    documents: RefCell<BTreeMap<String, DocumentRecord>>,
    texts: RefCell<BTreeMap<String, String>>,
    revisions: RefCell<BTreeMap<String, Vec<Revision>>>,
    now_initialized: RefCell<Option<bool>>,
    current_settings: RefCell<Option<Settings>>,
    /// Whether the fixture represents an initialized Library.
    pub initialized: bool,
    /// Settings returned by the fake (validated on every load).
    pub settings: Settings,
    /// Pages the fixture reports as no longer meeting their template.
    pub pages_off_template: u32,
    /// Template files the fixture reports as unreadable or invalid.
    pub template_invalid: Vec<String>,
    /// Files returned by `templates()`, in the order given.
    pub templates: Vec<LibraryText>,
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

    /// An initialized fixture whose template directory holds every
    /// bundled template, as `office.init` leaves it.
    #[must_use]
    pub fn with_bundled_templates() -> Self {
        Self {
            templates: BUNDLED
                .iter()
                .map(|bundled| LibraryText {
                    path: format!(".office/templates/{}", bundled.filename),
                    markdown: Some(bundled.markdown.to_string()),
                })
                .collect(),
            ..Self::initialized()
        }
    }

    /// Commits recorded for a Library-relative path (one per write).
    #[must_use]
    pub fn commits(&self, relative: &str) -> usize {
        self.revisions.borrow().get(relative).map_or(0, Vec::len)
    }

    fn is_now_initialized(&self) -> bool {
        self.now_initialized.borrow().unwrap_or(self.initialized)
    }

    fn ready(&self) -> Result<()> {
        if !self.is_now_initialized() {
            return Err(not_initialized_error());
        }
        self.current_settings
            .borrow()
            .as_ref()
            .unwrap_or(&self.settings)
            .validate()
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
        if let Some(existing) = self
            .documents
            .borrow()
            .values()
            .find(|doc| doc.id == record.id && doc.path != record.path)
        {
            return Err(Error::Library {
                code: "library_document_conflict",
                message: format!("document id {} is already at {}", record.id, existing.path),
            });
        }
        self.documents
            .borrow_mut()
            .insert(record.path.clone(), record.clone());
        self.texts
            .borrow_mut()
            .insert(record.path.clone(), text.to_string());
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

    fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        self.ready()?;
        Ok(self
            .documents
            .borrow()
            .values()
            .find(|doc| doc.id == id)
            .cloned())
    }

    fn backlinks(&self, id: &str) -> Result<Vec<Backlink>> {
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

    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| path.starts_with("knowledge/"))
            .map(|(path, markdown)| LibraryText {
                path: path.clone(),
                markdown: Some(markdown.clone()),
            })
            .collect())
    }

    fn templates(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self.templates.clone())
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
        if self.is_now_initialized() {
            self.ready()?;
        }
        Ok(self.is_now_initialized())
    }
    fn settings(&self) -> Result<Settings> {
        self.ready()?;
        Ok(self
            .current_settings
            .borrow()
            .as_ref()
            .unwrap_or(&self.settings)
            .clone())
    }
    fn set_setting(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let settings = self.settings()?.with_setting(key, value)?;
        *self.current_settings.borrow_mut() = Some(settings);
        Ok(())
    }
    fn initialize(&self, library_dir: Option<&str>) -> Result<InitOutcome> {
        if let Some(dir) = library_dir {
            let normalized = dir.replace('\\', "/");
            let allowed = normalized == "library"
                || normalized.starts_with("library/")
                || normalized.ends_with("/library")
                || normalized.contains("/library/");
            if !allowed || normalized.split('/').any(|part| part == "..") {
                return Err(Error::Library {
                    code: "library_dir_outside_workspace",
                    message: format!("library_dir {dir} is outside the granted Library root"),
                });
            }
        }
        if self.is_now_initialized() {
            return Ok(InitOutcome {
                already_initialized: true,
                created: Vec::new(),
            });
        }
        *self.now_initialized.borrow_mut() = Some(true);
        Ok(InitOutcome {
            already_initialized: false,
            created: vec![
                ".git".into(),
                ".gitignore".into(),
                ".office/config.yaml".into(),
                "artifacts/.gitkeep".into(),
                "index.sqlite".into(),
                "knowledge/domain/.gitkeep".into(),
                "knowledge/runbooks/.gitkeep".into(),
                "proposals/.gitkeep".into(),
                "sources/.gitkeep".into(),
            ],
        })
    }
    fn reindex(&self) -> Result<u32> {
        self.ready()?;
        let count = self.documents.borrow().len();
        u32::try_from(count).map_err(|_| Error::Library {
            code: "io_error",
            message: "document count overflow".into(),
        })
    }
    fn health(&self) -> Result<LibraryHealth> {
        if !self.is_now_initialized() {
            return Ok(LibraryHealth::uninitialized());
        }
        self.ready()?;
        let mut health = LibraryHealth {
            initialized: true,
            pages_off_template: self.pages_off_template,
            template_invalid: self.template_invalid.clone(),
            ..LibraryHealth::default()
        };
        for doc in self.documents.borrow().values() {
            health.documents_total = health.documents_total.saturating_add(1);
            match doc.kind {
                DocumentKind::Source => health.source = health.source.saturating_add(1),
                DocumentKind::Domain => health.domain = health.domain.saturating_add(1),
                DocumentKind::Runbook => {
                    health.runbook = health.runbook.saturating_add(1);
                }
                DocumentKind::Artifact => {
                    health.artifact = health.artifact.saturating_add(1);
                }
            }
        }
        Ok(health)
    }
}
