use super::*;

impl FakeLibrary {
    pub(super) fn read_video(
        &self,
        id: &str,
    ) -> Result<office_core::domain::source::video::VideoSource> {
        let source = self.read_media(id)?;
        if office_core::domain::source::parse_source_meta(&source.markdown).kind != "video" {
            return Err(office_core::domain::source::video::refusal(
                "not_a_video_source",
                "source is not a video",
            ));
        }
        Ok(source)
    }

    pub(super) fn source_media(
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

    pub(super) fn set_source_sensitivity(
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

    pub(super) fn read_media(
        &self,
        id: &str,
    ) -> Result<office_core::domain::source::video::VideoSource> {
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

    pub(super) fn evidence_files(&self, relative: &Path) -> Result<Vec<(String, Vec<u8>)>> {
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

    pub(super) fn source_sidecar(&self, id: &str, name: &str) -> Result<Option<String>> {
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

    pub(super) fn write_source_processing(
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
            decisions: Vec::new(),
            moved_from: None,
        };
        self.sources.borrow_mut().insert(id.into(), view.clone());
        Ok(view)
    }

    pub(super) fn mutate_video(
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
}
