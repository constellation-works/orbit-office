//! Deterministic meeting inbox, composed from the existing processing handlers.
use super::{Call, Result, Value, json, list, object};
use office_core::domain::source::{parse_source_meta, source_body, video};

pub(super) fn inbox(call: &Call<'_>) -> Result<Value> {
    if let Some(task) = call.input.get("task")
        && task.as_str().is_none_or(|s| s.is_empty())
    {
        return Err(office_core::Error::InvalidArgument(
            "task must be a nonempty string".into(),
        ));
    }
    let mut processed = Vec::new();
    // Complete enumeration, in path order, with no search-result cap.
    let mut sources = call.ports.library.source_documents()?;
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    for source in sources {
        let markdown = source.markdown.ok_or_else(|| {
            video::refusal(
                "invalid_source",
                "meeting inbox needs readable source metadata",
            )
        })?;
        let meta = parse_source_meta(&markdown);
        if meta.kind != "meeting"
            || call
                .ports
                .library
                .source_sidecar(&meta.id, "summary.md")?
                .is_some()
        {
            continue;
        }
        if let Some(state) = call
            .ports
            .library
            .source_sidecar(&meta.id, "transcript-state.json")?
        {
            let state: Value = serde_json::from_str(&state).map_err(|_| {
                video::refusal("invalid_source", "invalid meeting transcript state")
            })?;
            if state["complete"] != true {
                return Err(video::refusal(
                    "transcription_incomplete",
                    "meeting transcription stopped before its final audio window; finish it before summarizing",
                ));
            }
        }
        if source_body(&markdown).trim().is_empty() {
            if !meta
                .media_type
                .as_deref()
                .is_some_and(|mime| mime.starts_with("audio/") || mime.starts_with("video/"))
            {
                return Err(video::refusal(
                    "transcript_missing",
                    "meeting has no notes, transcript or audio",
                ));
            }
            let media = call.ports.library.read_media(&meta.id)?;
            let analysis = call.ports.models.analyze_video(&media.bytes, &0.into())?;
            if !analysis.has_audio
                || analysis.duration_ms == 0
                || analysis.duration_ms > 604_800_000
            {
                return Err(video::refusal(
                    "media_unavailable",
                    "meeting needs audio with a positive duration of at most seven days",
                ));
            }
            let input =
                json!({"id":meta.id,"duration_seconds":analysis.duration_ms.div_ceil(1000)});
            super::super::transcribe::run(&Call {
                input: object(&input)?,
                ports: call.ports,
                context: call.context,
            })?;
        }
        let mut input = json!({"id":meta.id,"profile":"meeting"});
        if let Some(task) = call.input.get("task") {
            input["task"] = task.clone();
        }
        super::run(&Call {
            input: object(&input)?,
            ports: call.ports,
            context: call.context,
        })?;
        processed.push(meta.id);
    }
    Ok(json!({"profile":"meeting","processed":processed}))
}

pub(super) fn summary(id: &str, result: &Value) -> Result<String> {
    let text = result["summary"]
        .as_str()
        .ok_or_else(super::invalid_reply)?;
    let mut out = format!(
        "# Meeting summary\n\n{}\n\n## Decisions\n",
        super::clean(text)
    );
    for decision in list(result, "decisions")? {
        let text = decision["text"].as_str().ok_or_else(super::invalid_reply)?;
        let cites = decision["cites"]
            .as_array()
            .ok_or_else(super::invalid_reply)?;
        let links = cites
            .iter()
            .map(|cite| {
                let anchor = cite.as_str().ok_or_else(super::invalid_reply)?;
                Ok(format!("[{id}{anchor}]({id}{anchor})"))
            })
            .collect::<Result<Vec<_>>>()?
            .join(" ");
        out.push_str(&format!("\n- {} {links}\n", super::clean(text)));
    }
    Ok(out)
}
