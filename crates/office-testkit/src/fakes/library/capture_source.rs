use super::*;

impl FakeLibrary {
    pub(super) fn capture_source(&self, request: &CaptureSourceRequest) -> Result<SourceOutcome> {
        office_core::domain::source::validate_capture_tags(&request.tags)?;
        office_core::domain::source::conversation::corrections::validate_capture(request)?;
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
                lines.push(entry.format(&request.captured_at)?);
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
            office_core::domain::source::conversation::corrections::validate_append(
                &view.body, &lines,
            )?;
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
                handoff: None,
            };
            let path = view.path.clone();
            let new_body = view.body.clone();
            drop(sources);
            if let Some(raw) = tagged_raw {
                let end = raw.find("\n---").map(|index| index + 4).unwrap_or(0);
                let markdown = format!("{}\n{}", &raw[..end], new_body);
                let markdown =
                    office_core::domain::source::conversation::recall::with_refs(&markdown)?;
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
        // As the store: plain text has no default kind, and ledgers take
        // entries only through the `decision` input.
        let kind = match explicit_kind {
            Some(kind) => kind,
            None if is_video_extension(&upload_ext) => SourceKind::Video,
            None if matches!(
                upload_ext.as_str(),
                "html" | "htm" | "docx" | "pptx" | "pdf" | "md" | "txt"
            ) =>
            {
                SourceKind::Doc
            }
            None if !request.entries.is_empty() => SourceKind::Conversation,
            None => return Err(office_core::domain::source::decisions::source_kind_required()),
        };
        if kind == SourceKind::Decisions {
            return Err(office_core::domain::source::decisions::decision_input_required());
        }
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
                lines.push(entry.format(&request.captured_at)?);
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
                handoff: None,
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
            decisions: Vec::new(),
            moved_from: None,
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
        let markdown = office_core::domain::source::conversation::recall::with_refs(&markdown)?;
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
            handoff: None,
        })
    }
}
