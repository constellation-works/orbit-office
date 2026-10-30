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
    bootstrap_batches: RefCell<BTreeMap<String, office_core::domain::bootstrap::Batch>>,
    bootstrap_digests: RefCell<BTreeMap<String, serde_json::Value>>,
    originals: RefCell<BTreeMap<String, Vec<u8>>>,
    artifact_files: RefCell<BTreeMap<String, Vec<u8>>>,
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
    /// Seed a validated digest sidecar by source id, without host I/O.
    pub fn insert_digest(&self, id: &str, value: serde_json::Value) -> Result<()> {
        office_core::domain::bootstrap::digest(&value)?;
        if !self
            .documents
            .borrow()
            .values()
            .any(|d| d.id == id && d.kind == DocumentKind::Source)
        {
            return Err(Error::Library {
                code: "source_not_found",
                message: "digest source is absent".into(),
            });
        }
        self.bootstrap_digests.borrow_mut().insert(id.into(), value);
        Ok(())
    }
    /// Seed an in-memory video with explicit source Markdown and original bytes.
    pub fn insert_video(&self, relative: &Path, markdown: &str, bytes: &[u8]) -> Result<()> {
        let meta = office_core::domain::source::parse_source_meta(markdown);
        self.write_document(relative, markdown)?;
        self.originals.borrow_mut().insert(meta.id, bytes.to_vec());
        Ok(())
    }

    /// Seed an allowlisted processing sidecar without host I/O.
    pub fn insert_source_sidecar(&self, id: &str, name: &str, text: &str) -> Result<()> {
        self.source_sidecar(id, name)?;
        let source = self.read_source(id)?;
        self.texts.borrow_mut().insert(
            Path::new(&source.path)
                .with_file_name(name)
                .to_string_lossy()
                .into_owned(),
            text.to_string(),
        );
        Ok(())
    }

    /// Seed a generated frame beside an existing source, without host I/O.
    pub fn insert_source_frame(&self, id: &str, name: &str, bytes: &[u8]) -> Result<()> {
        let source = self.read_source(id)?;
        let relative = office_core::domain::source::access::frame_file(name)?;
        self.artifact_files.borrow_mut().insert(
            Path::new(&source.path)
                .with_file_name(relative)
                .to_string_lossy()
                .into(),
            bytes.to_vec(),
        );
        Ok(())
    }

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

    /// Replace one existing document's revision history with explicit fixture
    /// timestamps. Does no host I/O and leaves other documents unchanged.
    pub fn set_history(&self, relative: &Path, revisions: Vec<Revision>) -> Result<()> {
        let record = self.read_document(relative)?;
        self.revisions.borrow_mut().insert(record.path, revisions);
        Ok(())
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
    fn set_shareable(&self, id: &str, shareable: bool) -> Result<DocumentRecord> {
        use office_core::domain::sharing::{self, SharingError};
        self.ready()?;
        let record = self.get(id)?.ok_or(SharingError::DocumentNotFound)?;
        let path = Path::new(&record.path);
        let markdown = self.read_text(path)?;
        let next = sharing::with_shareable(&markdown, shareable)?;
        if shareable {
            sharing::validate_sharing(&next, |id| self.get(id))?;
        }
        if next == markdown {
            return Ok(record);
        }
        self.write_document(path, &next)
    }

    fn write_reviewed_page(
        &self,
        relative: &Path,
        text: &str,
        expected: Option<&str>,
    ) -> Result<DocumentRecord> {
        self.ready()?;
        let current = self
            .texts
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned();
        let matches = match (current.as_deref(), expected) {
            (None, None) => true,
            (Some(current), Some(hash)) => sha256_hex(current.as_bytes()) == hash,
            (None, Some(_)) | (Some(_), None) => false,
        };
        if !matches {
            return Err(office_core::domain::proposal::refused(
                "proposal_revision_changed",
                "reviewed page destination changed before publication",
            ));
        }
        self.write_document(relative, text)
    }
    fn write_proposal(&self, relative: &Path, text: &str) -> Result<()> {
        self.ready()?;
        office_core::domain::proposal::validate_record_path(relative)?;
        self.texts
            .borrow_mut()
            .insert(relative.to_string_lossy().into_owned(), text.into());
        self.record_commit();
        Ok(())
    }

    fn bootstrap_snapshot(&self) -> Result<office_core::domain::bootstrap::Snapshot> {
        use office_core::domain::bootstrap::{Snapshot, Source};
        self.ready()?;
        let mut snapshot = Snapshot {
            batches: self.bootstrap_batches.borrow().clone(),
            ..Snapshot::default()
        };
        for file in self.source_documents()? {
            let markdown = file.markdown.ok_or(Error::LibraryUnavailable)?;
            let id = office_core::domain::source::parse_source_meta(&markdown).id;
            snapshot.sources.push(Source {
                path: file.path,
                markdown,
                digest: self.bootstrap_digests.borrow().get(&id).cloned(),
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        for file in self.knowledge_pages()? {
            let markdown = file.markdown.ok_or(Error::LibraryUnavailable)?;
            let id = DocumentRecord::from_markdown(Path::new(&file.path), &markdown).id;
            if seen.insert(id) {
                snapshot.pages.insert(file.path, markdown);
            }
        }
        Ok(snapshot)
    }
    fn mutate_batch(
        &self,
        revision: &str,
        mutation: office_core::domain::bootstrap::Mutation,
    ) -> Result<office_core::domain::bootstrap::Batch> {
        let snapshot = self.bootstrap_snapshot()?;
        let settings = self.settings()?;
        if snapshot.revision(&settings)? != revision {
            return Err(office_core::domain::source::video::refusal(
                "plan_changed",
                "Library changed during batch planning; preview again",
            ));
        }
        let write = office_core::domain::bootstrap::apply(&snapshot, &settings, mutation)?;
        if write.batches != snapshot.batches || !write.sources.is_empty() {
            for (path, markdown) in write.sources {
                self.write_document(Path::new(&path), &markdown)?;
                let id = office_core::domain::source::parse_source_meta(&markdown).id;
                if let Some(source) = self.sources.borrow_mut().get_mut(&id) {
                    source.sensitivity = "private".into();
                }
            }
            *self.bootstrap_batches.borrow_mut() = write.batches;
            self.record_commit();
        }
        Ok(write.batch)
    }
    fn read_video(&self, id: &str) -> Result<office_core::domain::source::video::VideoSource> {
        let source = self.read_media(id)?;
        if office_core::domain::source::parse_source_meta(&source.markdown).kind != "video" {
            return Err(office_core::domain::source::video::refusal(
                "not_a_video_source",
                "source is not a video",
            ));
        }
        Ok(source)
    }
    fn source_media(
        &self,
        id: &str,
        selection: &office_core::domain::source::access::MediaSelection,
    ) -> Result<office_core::domain::source::access::SourceMedia> {
        use office_core::domain::source::{access, video::refusal};
        let source = self.read_source(id)?;
        let markdown = self.read_text(Path::new(&source.path))?;
        let (name, media_type) = access::media_file(&markdown, selection, &self.settings()?)?;
        let originals = self.originals.borrow();
        let frames = self.artifact_files.borrow();
        let path = Path::new(&source.path)
            .with_file_name(&name)
            .to_string_lossy()
            .to_string();
        let bytes = match selection {
            access::MediaSelection::Original => originals.get(id),
            access::MediaSelection::Frame(_) => frames.get(&path),
        }
        .ok_or_else(|| {
            refusal(
                "source_media_unavailable",
                "source media file is unavailable",
            )
        })?;
        let content = if bytes.len() > access::MEDIA_LIMIT {
            None
        } else {
            if matches!(selection, access::MediaSelection::Original)
                && office_core::domain::source::parse_source_meta(&markdown)
                    .sha256
                    .as_deref()
                    != Some(sha256_hex(bytes).as_str())
            {
                return Err(refusal(
                    "plan_changed",
                    "original no longer matches its recorded digest",
                ));
            }
            Some(bytes.clone())
        };
        Ok(access::SourceMedia {
            name,
            media_type,
            bytes: u64::try_from(bytes.len())
                .map_err(|_| refusal("invalid_source", "media size overflow"))?,
            content,
        })
    }
    fn set_source_sensitivity(
        &self,
        id: &str,
        sensitivity: &str,
    ) -> Result<office_core::ports::SensitivityOutcome> {
        let source = self.read_source(id)?;
        let markdown = self.read_text(Path::new(&source.path))?;
        let unshared =
            sensitivity == "private" && office_core::domain::sharing::shareable(&markdown)?;
        let next = office_core::domain::source::access::with_sensitivity(&markdown, sensitivity)?;
        if next != markdown {
            self.write_document(Path::new(&source.path), &next)?;
            let updated = self.read_source(id)?;
            self.sources.borrow_mut().insert(id.into(), updated);
        }
        Ok(office_core::ports::SensitivityOutcome {
            source: self.read_source(id)?,
            unshared,
        })
    }
    fn read_media(&self, id: &str) -> Result<office_core::domain::source::video::VideoSource> {
        use office_core::domain::source::video::{VideoSource, refusal};
        self.ready()?;
        let doc = self
            .get(id)?
            .ok_or_else(|| refusal("source_not_found", "source was not found"))?;
        let markdown = self.read_text(Path::new(&doc.path))?;
        let meta = office_core::domain::source::parse_source_meta(&markdown);
        if meta.kind != "video" && meta.kind != "meeting" {
            return Err(refusal(
                "invalid_source_kind",
                "transcription needs a video or meeting source",
            ));
        }
        let bytes = self
            .originals
            .borrow()
            .get(id)
            .cloned()
            .ok_or_else(|| refusal("original_unavailable", "video original is unavailable"))?;
        if meta.sha256.as_deref() != Some(sha256_hex(&bytes).as_str()) {
            return Err(refusal(
                "plan_changed",
                "original no longer matches its recorded digest",
            ));
        }
        Ok(VideoSource {
            path: doc.path,
            markdown,
            bytes,
        })
    }
    fn evidence_files(&self, relative: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        use office_core::domain::sharing::sync::{SharedPath, classify};
        let dir = relative.parent().ok_or(Error::LibraryUnavailable)?;
        let mut files = BTreeMap::new();
        let texts = self.texts.borrow();
        let artifacts = self.artifact_files.borrow();
        let candidates = texts
            .iter()
            .map(|(path, text)| (path, text.as_bytes().to_vec()))
            .chain(artifacts.iter().map(|(path, bytes)| (path, bytes.clone())));
        for (path, bytes) in candidates {
            let path = Path::new(path);
            if path.parent() != Some(dir) || path == relative {
                continue;
            }
            let shared = format!("members/fake/{}", path.to_string_lossy());
            if let SharedPath::Companion { name, .. } = classify(&shared) {
                files.insert(name, bytes);
            }
        }
        Ok(files.into_iter().collect())
    }
    fn source_sidecar(&self, id: &str, name: &str) -> Result<Option<String>> {
        let source = self.get(id)?.ok_or_else(|| {
            office_core::domain::source::video::refusal("source_not_found", "source was not found")
        })?;
        office_core::domain::source::access::validate_sidecar(name)?;
        Ok(self
            .texts
            .borrow()
            .get(
                &Path::new(&source.path)
                    .with_file_name(name)
                    .to_string_lossy()
                    .to_string(),
            )
            .cloned())
    }
    fn write_source_processing(
        &self,
        id: &str,
        revision: &str,
        body: Option<&str>,
        derived: &office_core::ports::DerivedBy,
        files: &[(String, Vec<u8>)],
    ) -> Result<SourceView> {
        use office_core::domain::source::{
            parse_source_meta, processing_markdown, source_body, video::refusal,
        };
        let doc = self
            .get(id)?
            .ok_or_else(|| refusal("source_not_found", "source was not found"))?;
        let markdown = self.read_text(Path::new(&doc.path))?;
        if sha256_hex(markdown.as_bytes()) != revision {
            return Err(refusal("plan_changed", "source changed during processing"));
        }
        let (text, snapshot) = processing_markdown(&markdown, body, derived)?;
        let mut files = files.to_vec();
        if let Some(name) = snapshot {
            files.push((name, text.as_bytes().to_vec()));
        }
        for (name, bytes) in &files {
            if !office_core::domain::source::processing_file_name(name) {
                return Err(refusal("unsafe_path", "unsupported source processing file"));
            }
            let path = Path::new(&doc.path)
                .with_file_name(name)
                .to_string_lossy()
                .to_string();
            if name.starts_with('v')
                && self
                    .texts
                    .borrow()
                    .get(&path)
                    .is_some_and(|old| old.as_bytes() != bytes)
            {
                return Err(refusal(
                    "source_version_conflict",
                    "existing transcript version differs",
                ));
            }
        }
        for (name, bytes) in files {
            if name == "digest.json" {
                self.bootstrap_digests.borrow_mut().insert(
                    id.into(),
                    serde_json::from_slice(&bytes)
                        .map_err(|_| refusal("invalid_source", "digest is not JSON"))?,
                );
            }
            self.texts.borrow_mut().insert(
                Path::new(&doc.path)
                    .with_file_name(name)
                    .to_string_lossy()
                    .to_string(),
                String::from_utf8(bytes)
                    .map_err(|_| refusal("invalid_source", "sidecar is not UTF-8"))?,
            );
        }
        self.write_document(Path::new(&doc.path), &text)?;
        let meta = parse_source_meta(&text);
        let view = SourceView {
            id: meta.id,
            kind: meta.kind,
            title: meta.title,
            path: doc.path,
            body: source_body(&text).trim_end().into(),
            sensitivity: meta.sensitivity,
            version: meta.version,
            sha256: meta.sha256,
            bytes: meta.bytes,
            media_type: meta.media_type,
        };
        self.sources.borrow_mut().insert(id.into(), view.clone());
        Ok(view)
    }
    fn mutate_video(
        &self,
        id: &str,
        revision: &str,
        mutation: office_core::domain::source::video::VideoMutation,
    ) -> Result<office_core::domain::source::video::VideoState> {
        use office_core::domain::source::video::{apply, refusal};
        let source = self.read_video(id)?;
        if sha256_hex(source.markdown.as_bytes()) != revision {
            return Err(refusal("plan_changed", "video changed during processing"));
        }
        let write = apply(&source, &self.settings()?, mutation)?;
        if write.markdown == source.markdown && write.files.is_empty() {
            return Ok(write.state);
        }
        let mut record = self.write_document(Path::new(&source.path), &write.markdown)?;
        if let Some(view) = self.sources.borrow_mut().get_mut(id) {
            view.sensitivity = write.state.plan.sensitivity.clone();
            view.body = office_core::domain::source::source_body(&write.markdown);
        }
        let parent = Path::new(&source.path)
            .parent()
            .ok_or_else(|| refusal("invalid_video_source", "source path has no parent"))?;
        for (name, bytes) in write.files {
            let path = parent.join(&name).to_string_lossy().to_string();
            if name == "frames.md" {
                self.texts
                    .borrow_mut()
                    .insert(path.clone(), String::from_utf8_lossy(&bytes).into());
            }
            self.artifact_files.borrow_mut().insert(path, bytes);
        }
        if let Some(frames) = self
            .texts
            .borrow()
            .get(&parent.join("frames.md").to_string_lossy().to_string())
        {
            record.body.push_str("\n\n");
            record.body.push_str(frames);
        }
        self.documents
            .borrow_mut()
            .insert(record.path.clone(), record);
        Ok(write.state)
    }

    fn promote_artifact(
        &self,
        candidate: &office_core::domain::artifact::Artifact,
        files: &[office_core::domain::artifact::ArtifactFile],
    ) -> Result<office_core::domain::artifact::Artifact> {
        use office_core::domain::artifact::Artifact;
        self.ready()?;
        candidate.validate_bundle(files)?;
        let history = self
            .artifact_documents()?
            .into_iter()
            .map(|file| {
                let text = file.markdown.ok_or(Error::LibraryUnavailable)?;
                Ok(Artifact::from_document(&file.path, &text)?)
            })
            .collect::<Result<Vec<_>>>()?;
        let artifact = candidate.next_version(&history, |_| false)?;
        let path = artifact.path()?;
        self.write_document(Path::new(&path), &artifact.to_markdown()?)?;
        for file in files {
            self.artifact_files.borrow_mut().insert(
                format!("{}/{}", path.trim_end_matches("/artifact.md"), file.path),
                file.bytes.clone(),
            );
        }
        Ok(artifact)
    }
    fn read_artifact_file(&self, relative: &Path) -> Result<Vec<u8>> {
        self.ready()?;
        self.artifact_files
            .borrow()
            .get(&relative.to_string_lossy().to_string())
            .cloned()
            .ok_or_else(|| Error::Library {
                code: "io_error",
                message: format!("artifact file {} is absent", relative.display()),
            })
    }
    fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| {
                office_core::domain::knowledge::local_document_path(path).starts_with("artifacts/")
                    && path.ends_with("/artifact.md")
            })
            .map(|(path, text)| LibraryText {
                path: path.clone(),
                markdown: Some(text.clone()),
            })
            .collect())
    }

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
        if DocumentRecord::from_markdown(relative, text).shareable {
            office_core::domain::sharing::validate_sharing(text, |id| self.get(id))?;
        }
        let mut record = DocumentRecord::from_markdown(relative, text);
        if record.kind == DocumentKind::Source
            && let Some(digest) = self.bootstrap_digests.borrow().get(&record.id)
        {
            record.body.push_str("\n\n");
            record.body.push_str(&digest.to_string());
        }
        if record.kind == DocumentKind::Source {
            let summary_path = relative
                .with_file_name("summary.md")
                .to_string_lossy()
                .to_string();
            if let Some(summary) = self.texts.borrow().get(&summary_path) {
                record.body.push_str("\n\n");
                record.body.push_str(summary);
            }
        }
        if let Some(existing) =
            self.documents.borrow().values().find(|doc| {
                doc.id == record.id && doc.layer == record.layer && doc.path != record.path
            })
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

    fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
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
            .filter(|(path, _)| {
                office_core::domain::knowledge::local_document_path(path).starts_with("knowledge/")
            })
            .map(|(path, markdown)| LibraryText {
                path: path.clone(),
                markdown: Some(markdown.clone()),
            })
            .collect())
    }

    fn source_documents(&self) -> Result<Vec<LibraryText>> {
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
        office_core::domain::source::validate_capture_tags(&request.tags)?;
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
            let raw = self.texts.borrow().get(&view.path).cloned();
            let tagged_raw = raw
                .map(|raw| {
                    office_core::domain::source::merge_source_tags(&raw, &request.tags)
                        .map(|merged| merged.unwrap_or(raw))
                })
                .transpose()?;
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
            if let Some(raw) = tagged_raw {
                let end = raw.find("\n---").map(|index| index + 4).unwrap_or(0);
                let markdown = format!("{}\n{}", &raw[..end], new_body);
                let record = DocumentRecord::from_markdown(std::path::Path::new(&path), &markdown);
                self.documents.borrow_mut().insert(path.clone(), record);
                self.texts.borrow_mut().insert(path.clone(), markdown);
            }
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
            transcript_body = Some(vtt_to_body(&text)?);
        }
        if transcript_body.is_none() && upload_ext == "vtt" {
            let bytes = upload_bytes.as_deref().unwrap_or_default();
            let text = std::str::from_utf8(bytes).map_err(|_| {
                office_core::Error::InvalidArgument("upload must be UTF-8 WebVTT".into())
            })?;
            transcript_body = Some(vtt_to_body(text)?);
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
        if request.event.is_none()
            && let Some(sha_value) = &sha
            && let Some(existing) = self
                .sources
                .borrow()
                .values()
                .find(|s| {
                    s.sha256.as_ref() == Some(sha_value)
                        || sha256_hex(normalize_text(&s.body).as_bytes()) == *sha_value
                })
                .cloned()
        {
            let raw = self.texts.borrow().get(&existing.path).cloned();
            if let Some(raw) = raw
                && let Some(markdown) =
                    office_core::domain::source::merge_source_tags(&raw, &request.tags)?
            {
                let record =
                    DocumentRecord::from_markdown(std::path::Path::new(&existing.path), &markdown);
                self.documents
                    .borrow_mut()
                    .insert(existing.path.clone(), record);
                self.texts
                    .borrow_mut()
                    .insert(existing.path.clone(), markdown);
                self.record_commit();
            }
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
        let path = format!("sources/{}/{yyyy}/{mm}/{id}/source.md", kind.as_str());
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
        if let Some(bytes) = &upload_bytes {
            self.originals
                .borrow_mut()
                .insert(id.clone(), bytes.clone());
        }
        // Mirror into documents so search/health see the source.
        let original_name = format!("original.{upload_ext}");
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
            event: request.event.as_ref(),
            tags: &request.tags,
            sensitivity,
            original: match (&sha, bytes_len, &media_type) {
                (Some(sha), Some(bytes), Some(media)) => {
                    Some((original_name.as_str(), sha.as_str(), bytes, media.as_str()))
                }
                _ => None,
            },
            derived_transcription: derived,
            retention_days: settings.retention.get(kind.as_str()).copied().flatten(),
            body: &body,
        });
        if let Some(bytes) = upload_bytes {
            self.originals.borrow_mut().insert(id.clone(), bytes);
        }
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
        let doc = self.get(id)?.ok_or_else(|| {
            office_core::domain::source::video::refusal("source_not_found", "source was not found")
        })?;
        let text = self.read_text(Path::new(&doc.path))?;
        let meta = office_core::domain::source::parse_source_meta(&text);
        Ok(SourceView {
            id: meta.id,
            kind: meta.kind,
            title: meta.title,
            path: doc.path,
            body: office_core::domain::source::source_body(&text)
                .trim_end_matches('\n')
                .into(),
            sensitivity: meta.sensitivity,
            sha256: meta.sha256,
            bytes: meta.bytes,
            media_type: meta.media_type,
            version: meta.version,
        })
    }

    fn initialize(&self, library_dir: Option<&str>) -> Result<InitOutcome> {
        if let Some(dir) = library_dir {
            // The fake has no workspace root, so it cannot recognise an
            // absolute path under the granted `{workspace}/library` root the
            // way the adapter does. It accepts only a workspace-relative
            // `library` path and refuses every absolute one.
            let normalized = dir.replace('\\', "/");
            let parts: Vec<&str> = normalized
                .split('/')
                .filter(|part| !part.is_empty() && *part != ".")
                .collect();
            let allowed = !normalized.starts_with('/')
                && parts.first() == Some(&"library")
                && !parts.contains(&"..");
            if !allowed {
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
