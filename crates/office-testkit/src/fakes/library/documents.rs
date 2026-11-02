use super::*;

impl FakeLibrary {
    pub(super) fn set_shareable(&self, id: &str, shareable: bool) -> Result<DocumentRecord> {
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

    pub(super) fn write_reviewed_page(
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

    pub(super) fn write_proposal(&self, relative: &Path, text: &str) -> Result<()> {
        self.ready()?;
        office_core::domain::proposal::validate_record_path(relative)?;
        self.texts
            .borrow_mut()
            .insert(relative.to_string_lossy().into_owned(), text.into());
        self.record_commit();
        Ok(())
    }

    pub(super) fn move_proposal(&self, to: &Path, text: &str, from: &[String]) -> Result<()> {
        self.ready()?;
        office_core::domain::proposal::validate_record_path(to)?;
        for path in from {
            office_core::domain::proposal::validate_record_path(Path::new(path))?;
        }
        let to = to.to_string_lossy().into_owned();
        let mut texts = self.texts.borrow_mut();
        for path in from {
            if *path != to {
                texts.remove(path);
            }
        }
        texts.insert(to, text.into());
        drop(texts);
        self.record_commit();
        Ok(())
    }

    pub(super) fn proposal_pages(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| {
                office_core::domain::proposal::parse_record_path(Path::new(path.as_str())).is_ok()
            })
            .map(|(path, text)| LibraryText {
                path: path.clone(),
                markdown: Some(text.clone()),
            })
            .collect())
    }

    pub(super) fn bootstrap_snapshot(&self) -> Result<office_core::domain::bootstrap::Snapshot> {
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

    pub(super) fn mutate_batch(
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

    pub(super) fn promote_artifact(
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

    pub(super) fn read_artifact_file(&self, relative: &Path) -> Result<Vec<u8>> {
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

    pub(super) fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
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

    pub(super) fn read_document(&self, relative: &Path) -> Result<DocumentRecord> {
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

    pub(super) fn write_document(&self, relative: &Path, text: &str) -> Result<DocumentRecord> {
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
}
