use super::*;

impl Library for FakeLibrary {
    fn set_shareable(&self, id: &str, shareable: bool) -> Result<DocumentRecord> {
        FakeLibrary::set_shareable(self, id, shareable)
    }

    fn write_reviewed_page(
        &self,
        relative: &Path,
        text: &str,
        expected: Option<&str>,
    ) -> Result<DocumentRecord> {
        FakeLibrary::write_reviewed_page(self, relative, text, expected)
    }

    fn write_proposal(&self, relative: &Path, text: &str) -> Result<()> {
        FakeLibrary::write_proposal(self, relative, text)
    }

    fn move_proposal(&self, to: &Path, text: &str, from: &[String]) -> Result<()> {
        FakeLibrary::move_proposal(self, to, text, from)
    }

    fn proposal_pages(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::proposal_pages(self)
    }

    fn bootstrap_snapshot(&self) -> Result<office_core::domain::bootstrap::Snapshot> {
        FakeLibrary::bootstrap_snapshot(self)
    }

    fn mutate_batch(
        &self,
        revision: &str,
        mutation: office_core::domain::bootstrap::Mutation,
    ) -> Result<office_core::domain::bootstrap::Batch> {
        FakeLibrary::mutate_batch(self, revision, mutation)
    }

    fn read_video(&self, id: &str) -> Result<office_core::domain::source::video::VideoSource> {
        FakeLibrary::read_video(self, id)
    }

    fn source_media(
        &self,
        id: &str,
        selection: &office_core::domain::source::access::MediaSelection,
    ) -> Result<office_core::domain::source::access::SourceMedia> {
        FakeLibrary::source_media(self, id, selection)
    }

    fn set_source_sensitivity(
        &self,
        id: &str,
        sensitivity: &str,
    ) -> Result<office_core::ports::SensitivityOutcome> {
        FakeLibrary::set_source_sensitivity(self, id, sensitivity)
    }

    fn read_media(&self, id: &str) -> Result<office_core::domain::source::video::VideoSource> {
        FakeLibrary::read_media(self, id)
    }

    fn evidence_files(&self, relative: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        FakeLibrary::evidence_files(self, relative)
    }

    fn source_sidecar(&self, id: &str, name: &str) -> Result<Option<String>> {
        FakeLibrary::source_sidecar(self, id, name)
    }

    fn write_source_processing(
        &self,
        id: &str,
        revision: &str,
        body: Option<&str>,
        derived: &office_core::ports::DerivedBy,
        files: &[(String, Vec<u8>)],
    ) -> Result<SourceView> {
        FakeLibrary::write_source_processing(self, id, revision, body, derived, files)
    }

    fn mutate_video(
        &self,
        id: &str,
        revision: &str,
        mutation: office_core::domain::source::video::VideoMutation,
    ) -> Result<office_core::domain::source::video::VideoState> {
        FakeLibrary::mutate_video(self, id, revision, mutation)
    }

    fn promote_artifact(
        &self,
        candidate: &office_core::domain::artifact::Artifact,
        files: &[office_core::domain::artifact::ArtifactFile],
    ) -> Result<office_core::domain::artifact::Artifact> {
        FakeLibrary::promote_artifact(self, candidate, files)
    }

    fn read_artifact_file(&self, relative: &Path) -> Result<Vec<u8>> {
        FakeLibrary::read_artifact_file(self, relative)
    }

    fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::artifact_documents(self)
    }

    fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
        FakeLibrary::read_document(self, relative)
    }

    fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
        FakeLibrary::write_document(self, relative, text)
    }

    fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        FakeLibrary::search(self, query, filter)
    }

    fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        FakeLibrary::get(self, id)
    }

    fn backlinks(&self, id: &str) -> Result<Vec<Backlink>> {
        FakeLibrary::backlinks(self, id)
    }

    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::knowledge_pages(self)
    }

    fn source_documents(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::source_documents(self)
    }

    fn templates(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::templates(self)
    }

    fn history(&self, relative: &Path) -> Result<Vec<Revision>> {
        FakeLibrary::history(self, relative)
    }

    fn read_text(&self, relative: &Path) -> Result<String> {
        FakeLibrary::read_text(self, relative)
    }

    fn head_commit(&self) -> Result<Option<String>> {
        FakeLibrary::head_commit(self)
    }

    fn stage_attachment(&self, name: &str, text: &str) -> Result<String> {
        FakeLibrary::stage_attachment(self, name, text)
    }

    fn remove_staged(&self, staged: &str) -> Result<()> {
        FakeLibrary::remove_staged(self, staged)
    }

    fn scribe_files(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::scribe_files(self)
    }

    fn decision_ledgers(&self) -> Result<Vec<LibraryText>> {
        FakeLibrary::decision_ledgers(self)
    }
    fn append_decision(
        &self,
        decision: &office_core::domain::source::decisions::Decision,
        captured_at: &str,
    ) -> Result<office_core::ports::DecisionOutcome> {
        FakeLibrary::append_decision(self, decision, captured_at)
    }
    fn decision_precedent_scores(
        &self,
        _query: &str,
        _entries: &[(String, String)],
    ) -> Result<std::collections::BTreeMap<String, f64>> {
        Ok(self.decision_precedent_scores.clone())
    }

    fn write_decision_ledgers(
        &self,
        files: &[office_core::domain::source::decisions::PlannedFile],
    ) -> Result<()> {
        FakeLibrary::write_decision_ledgers(self, files)
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
        FakeLibrary::capture_source(self, request)
    }

    fn session_transcript(
        &self,
        path: &str,
        session_id: &str,
    ) -> Result<office_core::domain::source::conversation::Session> {
        FakeLibrary::session_transcript(self, path, session_id)
    }

    fn capture_session(
        &self,
        request: &office_core::domain::source::conversation::Request,
        captured_at: &str,
    ) -> Result<SourceOutcome> {
        FakeLibrary::capture_session(self, request, captured_at)
    }

    fn read_source(&self, id: &str) -> Result<SourceView> {
        FakeLibrary::read_source(self, id)
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
