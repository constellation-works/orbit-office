use super::*;

impl FakeLibrary {
    pub(super) fn session_transcript(
        &self,
        path: &str,
        session_id: &str,
    ) -> Result<office_core::domain::source::conversation::Session> {
        let mut session = self.session_fixture(path, session_id)?;
        if session.sensitivity == "private" {
            session.lines.clear();
        }
        Ok(session)
    }

    pub(super) fn capture_session(
        &self,
        request: &office_core::domain::source::conversation::Request,
        captured_at: &str,
    ) -> Result<SourceOutcome> {
        use office_core::domain::source::{conversation, parse_source_meta, video::refusal};
        let mut session = self.session_fixture(&request.path, &request.session_id)?;
        let entries = conversation::entries(&session, request)?;
        let mut matching = vec![];
        for doc in self.source_documents()? {
            let text = doc.markdown.ok_or(Error::LibraryUnavailable)?;
            conversation::validate_provenance(&text)?;
            let meta = office_core::domain::metrics::metadata(&text)?;
            if meta.get("session_id").and_then(serde_json::Value::as_str)
                == Some(&request.session_id)
            {
                matching.push((doc.path, text, meta));
            }
        }
        if matching.len() > 1 {
            return Err(refusal(
                "session_source_conflict",
                "multiple sources have this session id",
            ));
        }
        let (id, path, base, changed) = if let Some((path, text, meta)) = matching.first() {
            let source = parse_source_meta(text);
            if source.kind != "conversation"
                || source
                    .orchestrator
                    .as_ref()
                    .is_some_and(|provider| provider != &session.orchestrator)
            {
                return Err(refusal(
                    "session_source_conflict",
                    "session id belongs to incompatible source provenance",
                ));
            }
            if meta
                .get("transcript_sha256")
                .and_then(serde_json::Value::as_str)
                == Some(&session.transcript_sha256)
                && conversation::recall::with_context(text, &session, &request.context)?
                    == text.as_str()
            {
                return Ok(SourceOutcome {
                    id: source.id,
                    kind: source.kind,
                    title: source.title,
                    path: path.clone(),
                    sensitivity: source.sensitivity,
                    deduplicated: true,
                    version: source.version,
                    handoff: request.handoff.as_ref().map(|_| "no_plan_change".into()),
                    ..SourceOutcome::default()
                });
            }
            (
                source.id,
                path.clone(),
                text.clone(),
                meta.get("transcript_sha256")
                    .and_then(serde_json::Value::as_str)
                    != Some(&session.transcript_sha256),
            )
        } else {
            let at = &session.lines[0].timestamp;
            let base_id = source_id_base(
                at,
                &format!(
                    "session-{}",
                    &sha256_hex(request.session_id.as_bytes())[..12]
                ),
            );
            let id = unique_source_id(&base_id, |candidate| {
                self.documents.borrow().values().any(|d| d.id == candidate)
            });
            let path = format!(
                "sources/conversation/{}/{}/{id}/source.md",
                &at[..4],
                &at[5..7]
            );
            let text = render_source(&RenderSource {
                id: &id,
                kind: SourceKind::Conversation,
                title: "Conversation session",
                captured_at,
                captured_by: "transcript-extractor",
                origin: None,
                occurred_at: Some(at),
                participants: &[],
                event: None,
                tags: &[],
                sensitivity: Sensitivity::parse(&session.sensitivity).unwrap_or_default(),
                original: None,
                derived_transcription: None,
                retention_days: None,
                body: "",
            });
            (id, path, text, false)
        };
        let scrubbed_base = conversation::scrub_source(&base)?;
        if scrubbed_base != base {
            session.sensitivity = "private".into();
        }
        let base = scrubbed_base;
        let changed = changed
            && !(request.from_line.is_some() && conversation::prefix_grew(&session, &base)?);
        let text = conversation::merge(&base, &session, &entries, &session.sensitivity, changed)?;
        let (text, handoff) =
            conversation::with_handoff(request, &session, &base, text, captured_at)?;
        let text = conversation::recall::with_context(&text, &session, &request.context)?;
        if changed {
            let snapshot = Path::new(&path)
                .with_file_name(format!("v{}.md", parse_source_meta(&base).version))
                .to_string_lossy()
                .into_owned();
            if self
                .texts
                .borrow()
                .get(&snapshot)
                .is_some_and(|old| old != &base)
            {
                return Err(refusal("plan_changed", "immutable snapshot already exists"));
            }
            self.texts.borrow_mut().insert(snapshot, base);
        }
        self.write_document(Path::new(&path), &text)?;
        self.sources
            .borrow_mut()
            .insert(id.clone(), self.read_source(&id)?);
        Ok(SourceOutcome {
            id,
            kind: "conversation".into(),
            title: parse_source_meta(&text).title,
            path,
            sensitivity: session.sensitivity,
            deduplicated: false,
            version: parse_source_meta(&text).version,
            handoff,
            ..SourceOutcome::default()
        })
    }

    pub(super) fn read_source(&self, id: &str) -> Result<SourceView> {
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
            decisions: Vec::new(),
            moved_from: None,
        })
    }
}
