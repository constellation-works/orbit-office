use super::*;

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

    /// Place an SCR file, as another session's capture left it.
    pub fn insert_scribe_file(&self, path: &str, text: &str) {
        self.scribe_texts
            .borrow_mut()
            .insert(path.to_string(), text.to_string());
    }

    pub(super) fn record_commit(&self) {
        *self.commits.borrow_mut() += 1;
    }

    /// Commits recorded for a Library-relative path (one per write).
    #[must_use]
    pub fn commits(&self, relative: &str) -> usize {
        self.revisions.borrow().get(relative).map_or(0, Vec::len)
    }

    pub(super) fn is_now_initialized(&self) -> bool {
        self.now_initialized.borrow().unwrap_or(self.initialized)
    }

    pub(super) fn read_contained(&self, raw: &str) -> Result<Vec<u8>> {
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

    pub(super) fn session_fixture(
        &self,
        path: &str,
        id: &str,
    ) -> Result<office_core::domain::source::conversation::Session> {
        use office_core::domain::source::{conversation, video::refusal};
        self.ready()?;
        let raw = self.session_transcripts.get(path).ok_or_else(|| {
            refusal(
                "session_transcript_unavailable",
                "no fixture at this session path",
            )
        })?;
        let mut session = conversation::parse(raw, id)?;
        let existing_private = self.texts.borrow().values().any(|text| {
            office_core::domain::metrics::metadata(text).is_ok_and(|meta| {
                meta.get("session_id").and_then(serde_json::Value::as_str) == Some(id)
                    && meta.get("sensitivity").and_then(serde_json::Value::as_str)
                        == Some("private")
            })
        });
        if session.scrubbed
            || existing_private
            || self.settings()?.sensitivity.get("conversation") == Some(&Sensitivity::Private)
        {
            session.sensitivity = "private".into();
        }
        Ok(session)
    }

    pub(super) fn ready(&self) -> Result<()> {
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
