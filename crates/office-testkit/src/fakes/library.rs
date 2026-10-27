//! In-memory Library port with document, search and history behavior.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use office_common::Sensitivity;
use office_core::domain::knowledge::{
    Backlink, DocumentKind, DocumentRecord, LinkKind, Revision, SearchFilter, SearchHit,
};
use office_core::domain::settings::Settings;
use office_core::domain::source::{
    RenderSource, SourceKind, format_conversation_entry, is_video_extension,
    media_type_for_extension, normalize_text, render_source, sha256_hex, source_id_base,
    unique_source_id, vtt_to_body,
};
use office_core::domain::template::BUNDLED;
use office_core::ports::{
    CaptureSourceRequest, InitOutcome, Library, LibraryHealth, LibraryText, SourceOutcome,
    SourceView, not_initialized_error,
};
use office_core::{Error, Result};
use std::path::PathBuf;

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
    sources: RefCell<BTreeMap<String, SourceView>>,
    /// Canonical roots upload/transcript paths must stay inside. Empty
    /// means any existing file may be read (unit-test convenience); the
    /// real adapter always sets its workspace root.
    pub allowed_upload_roots: Vec<PathBuf>,
    /// Files returned by `templates()`, in the order given.
    pub templates: Vec<LibraryText>,
    commits: RefCell<usize>,
    staging: FakeStaging,
}

/// What `stage_attachment` wrote, shared across clones so a test keeps an
/// observer after the fake moves into boxed ports.
#[derive(Clone, Default)]
pub struct FakeStaging {
    state: Rc<RefCell<StagingState>>,
}

#[derive(Default)]
struct StagingState {
    written: BTreeMap<String, String>,
    removed: Vec<String>,
}

impl FakeStaging {
    /// The text staged at `path`, kept after removal so a test can read
    /// what an artifact copy carried.
    #[must_use]
    pub fn text(&self, path: &str) -> Option<String> {
        self.state.borrow().written.get(path).cloned()
    }

    /// Every staged path, in path order.
    #[must_use]
    pub fn written(&self) -> Vec<String> {
        self.state.borrow().written.keys().cloned().collect()
    }

    /// Staged paths not yet removed.
    #[must_use]
    pub fn live(&self) -> Vec<String> {
        let state = self.state.borrow();
        state
            .written
            .keys()
            .filter(|path| !state.removed.contains(path))
            .cloned()
            .collect()
    }
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

    /// An observer of attachment staging; clones share the log.
    #[must_use]
    pub fn staging(&self) -> FakeStaging {
        self.staging.clone()
    }

    fn record_commit(&self) {
        *self.commits.borrow_mut() += 1;
    }

    /// Commits recorded for a Library-relative path (one per write).
    #[must_use]
    pub fn commits(&self, relative: &str) -> usize {
        self.revisions.borrow().get(relative).map_or(0, Vec::len)
    }

    fn is_now_initialized(&self) -> bool {
        self.now_initialized.borrow().unwrap_or(self.initialized)
    }

    fn read_contained(&self, raw: &str) -> Result<Vec<u8>> {
        let path = std::path::Path::new(raw);
        let canonical = std::fs::canonicalize(path).map_err(|_| Error::Library {
            code: "upload_not_found",
            message: format!("upload path {raw} could not be read"),
        })?;
        if !self.allowed_upload_roots.is_empty() {
            let inside = self.allowed_upload_roots.iter().any(|root| {
                std::fs::canonicalize(root).is_ok_and(|root| canonical.starts_with(root))
            });
            if !inside {
                return Err(Error::Library {
                    code: "upload_outside_allowed_roots",
                    message: format!("upload path {raw} is outside the allowed roots"),
                });
            }
        }
        std::fs::read(&canonical).map_err(|_| Error::Library {
            code: "upload_not_found",
            message: format!("upload path {raw} could not be read"),
        })
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
        self.record_commit();
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
    fn read_text(&self, relative: &Path) -> Result<String> {
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

    fn head_commit(&self) -> Result<Option<String>> {
        self.ready()?;
        let commits = *self.commits.borrow();
        Ok((commits > 0).then(|| format!("fake-head-{commits}")))
    }

    fn stage_attachment(&self, name: &str, text: &str) -> Result<String> {
        self.ready()?;
        let mut state = self.staging.state.borrow_mut();
        let path = format!("staging/{}/{name}", state.written.len() + 1);
        state.written.insert(path.clone(), text.to_string());
        Ok(path)
    }

    fn remove_staged(&self, staged: &str) -> Result<()> {
        let mut state = self.staging.state.borrow_mut();
        if !state.written.contains_key(staged) {
            return Err(Error::Library {
                code: "unsafe_path",
                message: format!("{staged} is not a staged attachment"),
            });
        }
        state.removed.push(staged.to_string());
        Ok(())
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
    fn record_route_check(
        &self,
        endpoint: &str,
        model: &str,
        capabilities: &[office_core::domain::endpoint::Capability],
        verified_at: &str,
    ) -> Result<()> {
        let settings =
            self.settings()?
                .with_route_check(endpoint, model, capabilities, verified_at)?;
        *self.current_settings.borrow_mut() = Some(settings);
        Ok(())
    }
    fn capture_source(&self, request: &CaptureSourceRequest) -> Result<SourceOutcome> {
        self.ready()?;
        let settings = self.settings()?;
        if let Some(target) = &request.append_to {
            if request.entries.is_empty() {
                return Err(Error::InvalidArgument(
                    "capture append_to requires at least one entry".to_string(),
                ));
            }
            let mut lines = Vec::new();
            for entry in &request.entries {
                lines.push(format_conversation_entry(
                    &request.captured_at,
                    &entry.kind,
                    &entry.text,
                )?);
            }
            let mut sources = self.sources.borrow_mut();
            let view = sources.get_mut(target).ok_or_else(|| Error::Library {
                code: "source_not_found",
                message: format!("source {target} was not found"),
            })?;
            if view.kind != "conversation" {
                return Err(Error::Library {
                    code: "not_a_conversation_source",
                    message: format!("source {target} is not a conversation source"),
                });
            }
            if !view.body.is_empty() && !view.body.ends_with('\n') {
                view.body.push('\n');
            }
            view.body.push_str(&lines.join("\n"));
            view.body.push('\n');
            let outcome = SourceOutcome {
                id: view.id.clone(),
                kind: view.kind.clone(),
                title: view.title.clone(),
                path: view.path.clone(),
                sha256: view.sha256.clone(),
                bytes: view.bytes,
                media_type: view.media_type.clone(),
                sensitivity: view.sensitivity.clone(),
                deduplicated: false,
                appended: lines.len(),
                version: view.version,
            };
            let path = view.path.clone();
            let new_body = view.body.clone();
            drop(sources);
            // Mirror the appended body into `documents`, as the create path
            // does, so `search` and `read_document` see appended entries.
            // Other record fields keep the frontmatter parsed at create.
            if let Some(record) = self.documents.borrow_mut().get_mut(&path) {
                record.body = new_body.trim().to_string();
            }
            let mut revisions = self.revisions.borrow_mut();
            let history = revisions.entry(path.clone()).or_default();
            history.insert(
                0,
                Revision {
                    commit: format!("fake-{}", history.len() + 1),
                    author: "orbit-office".into(),
                    email: "office@orbit.local".into(),
                    authored_at: request.captured_at.clone(),
                    message: format!("Update {path}"),
                },
            );
            self.record_commit();
            return Ok(outcome);
        }
        // Create path.
        let mut upload_bytes: Option<Vec<u8>> = None;
        let mut upload_ext = String::new();
        let mut upload_stem = String::new();
        if let Some(raw) = &request.upload_path {
            let bytes = self.read_contained(raw)?;
            let path = std::path::Path::new(raw);
            upload_ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            upload_stem = path
                .file_stem()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            upload_bytes = Some(bytes);
        }
        let mut transcript_body: Option<String> = None;
        if let Some(raw) = &request.transcript_path {
            if !raw.to_ascii_lowercase().ends_with(".vtt") {
                return Err(Error::InvalidArgument(
                    "transcript_path must name a .vtt file".to_string(),
                ));
            }
            let bytes = self.read_contained(raw)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                Error::InvalidArgument("transcript_path must be UTF-8 WebVTT".to_string())
            })?;
            transcript_body = Some(vtt_to_body(&text));
        }
        let explicit_kind = match &request.kind {
            Some(k) => Some(SourceKind::parse(k).ok_or_else(|| Error::Library {
                code: "invalid_source_kind",
                message: format!("unknown source kind {k:?}"),
            })?),
            None => None,
        };
        let kind = explicit_kind.unwrap_or_else(|| {
            if is_video_extension(&upload_ext) {
                SourceKind::Video
            } else if request.entries.is_empty() && request.text.is_some() {
                SourceKind::Note
            } else if !request.entries.is_empty() {
                SourceKind::Conversation
            } else {
                SourceKind::Note
            }
        });
        let (body, sha, bytes_len, media_type, derived) = if let Some(body) = transcript_body {
            let sha = upload_bytes.as_ref().map(|b| sha256_hex(b));
            (
                body,
                sha,
                upload_bytes.as_ref().map(|b| b.len() as u64),
                upload_bytes.as_ref().map(|_| {
                    media_type_for_extension(&upload_ext)
                        .unwrap_or("application/octet-stream")
                        .to_string()
                }),
                Some("vtt-import"),
            )
        } else if let Some(bytes) = &upload_bytes {
            let media = media_type_for_extension(&upload_ext)
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = if upload_ext == "txt" || upload_ext == "md" {
                normalize_text(&String::from_utf8_lossy(bytes))
            } else {
                String::new()
            };
            (
                body,
                Some(sha256_hex(bytes)),
                Some(bytes.len() as u64),
                Some(media),
                None,
            )
        } else if kind == SourceKind::Conversation && !request.entries.is_empty() {
            let mut lines = Vec::new();
            for entry in &request.entries {
                lines.push(format_conversation_entry(
                    &request.captured_at,
                    &entry.kind,
                    &entry.text,
                )?);
            }
            let body = lines.join("\n");
            let sha = sha256_hex(normalize_text(&body).as_bytes());
            (body, Some(sha), None, None, None)
        } else {
            let text = request.text.clone().unwrap_or_default();
            if text.trim().is_empty() && upload_bytes.is_none() {
                return Err(Error::InvalidArgument(
                    "capture requires text, upload_path, entries or append_to".to_string(),
                ));
            }
            let normalized = normalize_text(&text);
            (
                normalized.clone(),
                Some(sha256_hex(normalized.as_bytes())),
                None,
                None,
                None,
            )
        };
        if let Some(sha_value) = &sha
            && let Some(existing) = self
                .sources
                .borrow()
                .values()
                .find(|s| s.sha256.as_ref() == Some(sha_value))
                .cloned()
        {
            return Ok(SourceOutcome {
                id: existing.id,
                kind: existing.kind,
                title: existing.title,
                path: existing.path,
                sha256: existing.sha256,
                bytes: existing.bytes,
                media_type: existing.media_type,
                sensitivity: existing.sensitivity,
                deduplicated: true,
                appended: 0,
                version: existing.version,
            });
        }
        let title = request
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| {
                if !upload_stem.is_empty() {
                    upload_stem.replace(['-', '_'], " ")
                } else {
                    body.lines()
                        .next()
                        .unwrap_or(kind.as_str())
                        .chars()
                        .take(60)
                        .collect()
                }
            });
        let base = source_id_base(&request.captured_at, &title);
        let id = {
            let sources = self.sources.borrow();
            unique_source_id(&base, |candidate| sources.contains_key(candidate))
        };
        let date = &request.captured_at;
        let (yyyy, mm) = if date.len() >= 7 {
            (&date[0..4], &date[5..7])
        } else {
            ("1970", "01")
        };
        let path = format!("sources/{yyyy}/{mm}/{id}/source.md");
        let sensitivity = match &request.sensitivity {
            Some(s) => Sensitivity::parse(s).ok_or_else(|| {
                Error::InvalidArgument("sensitivity must be normal or private".to_string())
            })?,
            None => settings
                .sensitivity
                .get(kind.as_str())
                .copied()
                .unwrap_or_default(),
        };
        let view = SourceView {
            id: id.clone(),
            kind: kind.as_str().to_string(),
            title: title.clone(),
            path: path.clone(),
            body: body.clone(),
            sensitivity: sensitivity.as_str().to_string(),
            sha256: sha.clone(),
            bytes: bytes_len,
            media_type: media_type.clone(),
            version: 1,
        };
        self.sources.borrow_mut().insert(id.clone(), view);
        // Mirror into documents so search/health see the source.
        let markdown = render_source(&RenderSource {
            id: &id,
            kind,
            title: &title,
            captured_at: &request.captured_at,
            captured_by: request
                .captured_by
                .as_deref()
                .unwrap_or(if upload_bytes.is_some() {
                    "upload"
                } else {
                    "paste"
                }),
            origin: request.origin.as_deref(),
            occurred_at: request.occurred_at.as_deref(),
            participants: &request.participants,
            sensitivity,
            original: None,
            derived_transcription: derived,
            retention_days: settings.retention.get(kind.as_str()).copied().flatten(),
            body: &body,
        });
        let record = DocumentRecord::from_markdown(std::path::Path::new(&path), &markdown);
        self.documents.borrow_mut().insert(path.clone(), record);
        self.texts.borrow_mut().insert(path.clone(), markdown);
        self.record_commit();
        let mut revisions = self.revisions.borrow_mut();
        let history = revisions.entry(path.clone()).or_default();
        history.insert(
            0,
            Revision {
                commit: format!("fake-{}", history.len() + 1),
                author: "orbit-office".into(),
                email: "office@orbit.local".into(),
                authored_at: request.captured_at.clone(),
                message: format!("Update {path}"),
            },
        );
        Ok(SourceOutcome {
            id,
            kind: kind.as_str().to_string(),
            title,
            path,
            sha256: sha,
            bytes: bytes_len,
            media_type,
            sensitivity: sensitivity.as_str().to_string(),
            deduplicated: false,
            appended: 0,
            version: 1,
        })
    }
    fn read_source(&self, id: &str) -> Result<SourceView> {
        self.ready()?;
        self.sources
            .borrow()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::Library {
                code: "source_not_found",
                message: format!("source {id} was not found"),
            })
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
