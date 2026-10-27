//! Deterministic KT plans, digest-bound consent and the frame-call ledger.

use super::{parse_source_meta, sha256_hex, source_body};
use crate::domain::endpoint::Capability;
use crate::domain::settings::Settings;
use crate::ports::DerivedBy;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Maximum Hamming distance considered a near duplicate.
pub const DUPLICATE_DISTANCE: u32 = 5;
/// Scene metadata with the hash of ffmpeg's 9 by 8 grayscale thumbnail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoFrame {
    /// Milliseconds into the recording.
    pub time_ms: u64,
    /// Scene score scaled by one million.
    pub score: u32,
    /// 64-bit difference hash.
    pub dhash: u64,
}
/// Local media inspection, with no inference calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoAnalysis {
    /// Duration rounded up to milliseconds.
    pub duration_ms: u64,
    /// Whether ffprobe found an audio stream.
    pub has_audio: bool,
    /// Ordered or unordered scene candidates.
    pub candidates: Vec<VideoFrame>,
}
/// Counts for each fixed processing step.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoCalls {
    /// Audio chunks, at most ten minutes each; zero with a VTT.
    pub audio: usize,
    /// One image request per frame.
    pub frames: usize,
    /// One extraction, or 15-minute windows plus a merge.
    pub extract: usize,
}
/// Cost/route preview. The digest covers every field except the digest itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoPlan {
    /// Content digest approved by the user.
    pub digest: String,
    /// Source identity.
    pub id: String,
    /// Original's actual content digest.
    pub original_sha256: String,
    /// Transcript content digest.
    pub transcript_sha256: String,
    /// Validated policy and route snapshot digest.
    pub settings_digest: String,
    /// Normal or private, fixed at preview.
    pub sensitivity: String,
    /// Milliseconds, rounded up by ffprobe.
    pub duration_ms: u64,
    /// Whether audio exists and needs transcription.
    pub transcribe_audio: bool,
    /// Deduplicated selected frames, in time order.
    pub frames: Vec<VideoFrame>,
    /// Fixed upper bounds for each call group.
    pub counts: VideoCalls,
    /// Route for audio, frames and extract, with endpoint/model for private.
    pub routes: Value,
    /// Null when any applicable rate or currency is unset.
    pub estimate: Value,
    /// Missing applicable rate keys; empty when priced.
    pub rates_unset: Vec<String>,
}
/// A durable frame reading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameReading {
    /// Index into the confirmed frame list.
    pub index: usize,
    /// Model's textual reading.
    pub text: String,
    /// Model route.
    pub route: String,
    /// Model name.
    pub model: String,
}
/// Consent and actual calls live beside the preview in source frontmatter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoState {
    /// Audio replies durably published, excluding failed or uncertain requests.
    #[serde(default)]
    pub audio_completed: usize,
    /// Digest of the transcript produced under this plan, preserving its approved input.
    #[serde(default)]
    pub transcription_sha256: Option<String>,
    /// Preview that is or will be approved.
    pub plan: VideoPlan,
    /// Timestamp of the first matching confirmation.
    pub confirmed_at: Option<String>,
    /// Actual calls, including failed/uncertain requests.
    pub calls: VideoCalls,
    /// Number of successfully persisted readings.
    pub frames_read: usize,
    /// Completed readings, retained across partial failures.
    pub readings: Vec<FrameReading>,
}
/// Contained original bytes and source metadata supplied by the Library port.
#[derive(Clone, Debug)]
pub struct VideoSource {
    /// Source document path.
    pub path: String,
    /// Source Markdown, preserving unrelated fields and body.
    pub markdown: String,
    /// Original bytes, checked against source.original.sha256.
    pub bytes: Vec<u8>,
}
/// One compare-and-swap Library mutation. Call reservations are durable before inference.
pub enum VideoMutation {
    /// Durably consume the next audio or extraction call before inference.
    ReserveProcessing {
        /// Approved digest.
        digest: String,
        /// `audio` or `extract`.
        group: String,
        /// Zero-based next call.
        index: usize,
    },
    /// Save a preview or confirm the digest shown by the caller.
    Plan {
        /// New preview.
        plan: Box<VideoPlan>,
        /// Digest to confirm, or no confirmation.
        confirm_digest: Option<String>,
        /// Current time from the clock port.
        at: String,
    },
    /// Consume exactly the next planned image-call slot.
    Reserve {
        /// Approved digest.
        digest: String,
        /// Next frame index.
        index: usize,
    },
    /// Publish one result and its 640-pixel image.
    Frame {
        /// Approved digest.
        digest: String,
        /// Reserved frame index.
        index: usize,
        /// PNG bytes.
        image: Vec<u8>,
        /// Reading text.
        text: String,
        /// Route/model actually used.
        derived_by: DerivedBy,
    },
}
/// Changed files, relative to the source directory.
pub struct VideoWrite {
    /// Updated source Markdown.
    pub markdown: String,
    /// Derived Markdown and ignored images.
    pub files: Vec<(String, Vec<u8>)>,
    /// Updated ledger returned to the caller.
    pub state: VideoState,
}
/// A stable, actionable video refusal.
#[must_use]
pub fn refusal(code: &'static str, message: &str) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}
/// Read the bounded YAML video mapping. Duplicate/malformed fields fail closed.
pub fn state(markdown: &str) -> Result<Option<VideoState>> {
    let header = markdown
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .ok_or_else(|| refusal("invalid_video_source", "video source needs frontmatter"))?;
    let mut fields = header
        .lines()
        .filter_map(|line| line.strip_prefix("video: "));
    let result = fields.next().map(decode_state).transpose()?;
    if fields.next().is_some() {
        return Err(refusal("invalid_video_plan", "duplicate video ledger"));
    }
    Ok(result)
}
fn decode_state(raw: &str) -> Result<VideoState> {
    let fields = crate::domain::knowledge::frontmatter::parse(&format!("video: {raw}"))?;
    let value = &fields[0].1;
    let invalid = || refusal("invalid_video_plan", "video ledger is malformed");
    let plan: VideoPlan =
        serde_json::from_str(value["plan"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let unsigned = |value: &Value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|n| n.parse::<u64>().ok()))
            .ok_or_else(invalid)
    };
    let count = |value: &Value| usize::try_from(unsigned(value)?).map_err(|_| invalid());
    let calls = VideoCalls {
        audio: count(&value["calls"]["audio"])?,
        frames: count(&value["calls"]["frames"])?,
        extract: count(&value["calls"]["extract"])?,
    };
    let readings = serde_json::from_str(value["readings"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let confirmed_at = if value["confirmed_at"].is_null() {
        None
    } else {
        Some(value["confirmed_at"].as_str().ok_or_else(invalid)?.into())
    };
    let frames_read = count(&value["frames_read"])?;
    if value["plan_digest"] != plan.digest
        || unsigned(&value["duration_s"])? != plan.duration_ms.div_ceil(1000)
    {
        return Err(invalid());
    }
    let state = VideoState {
        audio_completed: value
            .get("audio_completed")
            .map(count)
            .transpose()?
            .unwrap_or(0),
        transcription_sha256: value
            .get("transcription_sha256")
            .and_then(Value::as_str)
            .map(str::to_string),
        plan,
        confirmed_at,
        calls,
        frames_read,
        readings,
    };
    if state.plan.counts.frames != state.plan.frames.len()
        || state.frames_read != state.readings.len()
        || state.calls.frames > state.plan.counts.frames
        || state.frames_read > state.calls.frames
        || state.calls.audio > state.plan.counts.audio
        || state.audio_completed > state.calls.audio
        || state.calls.extract > state.plan.counts.extract
        || state
            .readings
            .iter()
            .enumerate()
            .any(|(index, reading)| reading.index != index)
        || (state.confirmed_at.is_none() && state.calls != VideoCalls::default())
    {
        return Err(invalid());
    }
    Ok(state)
}
fn encode_state(state: &VideoState) -> Result<String> {
    let invalid = || refusal("invalid_video_plan", "cannot encode ledger");
    Ok(flow(
        &json!({"duration_s":state.plan.duration_ms.div_ceil(1000),"plan_digest":state.plan.digest,
        "plan":serde_json::to_string(&state.plan).map_err(|_|invalid())?,
        "confirmed_at":state.confirmed_at,"calls":state.calls,"frames_read":state.frames_read,
        "readings":serde_json::to_string(&state.readings).map_err(|_|invalid())?, "transcription_sha256":state.transcription_sha256,"audio_completed":state.audio_completed}),
    ))
}
// Emit the Library's supported YAML flow subset (plain keys plus JSON scalars).
// Larger nested state is stored as quoted JSON so unrelated frontmatter readers
// can still read participants, tags and provenance within their depth bound.
pub(super) fn flow(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(key, value)| format!("{key}: {}", flow(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(flow).collect::<Vec<_>>().join(", ")
        ),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
    }
}
/// Record the generated transcript hash without changing the approved plan digest.
pub(super) fn with_transcript(markdown: &str, body: &str) -> Result<Option<String>> {
    if super::parse_source_meta(markdown).kind == "video" && state(markdown)?.is_none() {
        return Err(refusal(
            "plan_unconfirmed",
            "preview and confirm the video plan first",
        ));
    }
    state(markdown)?
        .map(|mut state| {
            if state.confirmed_at.is_none()
                || state.calls.audio != state.audio_completed.saturating_add(1)
            {
                return Err(refusal(
                    "plan_exceeded",
                    "transcript has no outstanding approved audio reservation",
                ));
            }
            state.audio_completed += 1;
            state.transcription_sha256 = Some(sha256_hex(body.as_bytes()));
            encode_state(&state)
        })
        .transpose()
}
/// Policy identity excludes unrelated Library settings and includes private capabilities.
pub fn settings_digest(settings: &Settings) -> Result<String> {
    let bytes = serde_json::to_vec(&json!({"video":settings.video,"costs":settings.costs,"open_weights":settings.open_weights}))
        .map_err(|_| refusal("invalid_video_plan", "cannot encode video settings"))?;
    Ok(sha256_hex(&bytes))
}
/// Select spaced, deduplicated candidates, then the highest score in equal-duration buckets.
#[must_use]
pub fn select_frames(analysis: &VideoAnalysis, per_hour: u32) -> Vec<VideoFrame> {
    let cap = (analysis.duration_ms.saturating_mul(u64::from(per_hour)) / 3_600_000).max(1);
    let mut candidates = analysis.candidates.clone();
    candidates.sort_by_key(|frame| frame.time_ms);
    let mut kept: Vec<VideoFrame> = Vec::new();
    for frame in candidates {
        if frame.time_ms >= analysis.duration_ms {
            continue;
        }
        if kept.last().is_some_and(|last| {
            frame.time_ms.saturating_sub(last.time_ms) < 10_000
                || (frame.dhash ^ last.dhash).count_ones() <= DUPLICATE_DISTANCE
        }) {
            continue;
        }
        kept.push(frame);
    }
    if u64::try_from(kept.len()).unwrap_or(u64::MAX) > cap {
        let mut buckets = std::collections::BTreeMap::<u64, VideoFrame>::new();
        for frame in kept {
            let bucket = frame.time_ms.saturating_mul(cap) / analysis.duration_ms;
            if buckets
                .get(&bucket)
                .is_none_or(|old| frame.score > old.score)
            {
                buckets.insert(bucket, frame);
            }
        }
        kept = Vec::new();
        for frame in buckets.into_values() {
            if kept
                .last()
                .is_none_or(|last| (frame.dhash ^ last.dhash).count_ones() > DUPLICATE_DISTANCE)
            {
                kept.push(frame);
            }
        }
    }
    kept
}
/// Construct the exact cost preview without spending model calls.
pub fn plan(
    source: &VideoSource,
    analysis: &VideoAnalysis,
    settings: &Settings,
    sensitivity: &str,
) -> Result<VideoPlan> {
    if analysis.duration_ms == 0 || analysis.duration_ms > 604_800_000 {
        return Err(refusal(
            "invalid_video_source",
            "video duration must be positive and at most seven days",
        ));
    }
    let meta = parse_source_meta(&source.markdown);
    let (header, _) = crate::domain::knowledge::frontmatter::split(&source.markdown)?;
    let metadata = crate::domain::knowledge::frontmatter::parse(header)?;
    let imported_vtt = metadata
        .iter()
        .find(|(key, _)| key == "derived_by")
        .is_some_and(|(_, value)| value["transcription"] == "vtt-import");
    let transcribe_audio = analysis.has_audio && !imported_vtt;
    let frames = select_frames(analysis, settings.video.max_frames_per_hour);
    let counts = VideoCalls {
        audio: if transcribe_audio {
            usize::try_from(analysis.duration_ms.div_ceil(600_000)).unwrap_or(usize::MAX)
        } else {
            0
        },
        frames: frames.len(),
        extract: if analysis.duration_ms <= 900_000 {
            1
        } else {
            usize::try_from(analysis.duration_ms.div_ceil(900_000)).unwrap_or(usize::MAX) + 1
        },
    };
    let route = if sensitivity == "private" {
        let block = settings.open_weights.as_ref();
        let missing = Capability::ALL
            .into_iter()
            .filter(|cap| {
                let needed = match cap {
                    Capability::Text => false,
                    Capability::Image => !frames.is_empty(),
                    Capability::Audio => transcribe_audio,
                };
                needed && block.is_none_or(|block| !block.accepts(*cap))
            })
            .map(Capability::as_str)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(refusal(
                "private_media_unsupported",
                &format!(
                    "private endpoint lacks recorded capabilities: {}",
                    missing.join(", ")
                ),
            ));
        }
        let block = block.ok_or_else(|| {
            refusal(
                "private_endpoint_unverified",
                "private endpoint has not passed a text capability check",
            )
        })?;
        if !block.accepts(Capability::Text) {
            return Err(refusal(
                "private_endpoint_unverified",
                "private endpoint has not passed its recorded text capability check",
            ));
        }
        json!({"route":format!("private:{}",block.kind.as_str()),"endpoint":block.endpoint,"model":block.model})
    } else {
        json!({"route":"hosted","endpoint":"https://api.meta.ai/v1","model":"muse-spark-1.3"})
    };
    let mut rates_unset = Vec::new();
    let mut rate = |name: &str, value: &Option<serde_json::Number>, needed: bool| {
        if needed && value.is_none() {
            rates_unset.push(format!("costs.{name}"));
        }
        value
            .as_ref()
            .and_then(serde_json::Number::as_f64)
            .unwrap_or(0.0)
    };
    let audio_rate = rate(
        "audio_minute",
        &settings.costs.audio_minute,
        transcribe_audio,
    );
    let image_rate = rate("image", &settings.costs.image, !frames.is_empty());
    let text_rate = rate("text_1k_tokens", &settings.costs.text_1k_tokens, true);
    if settings.costs.currency.is_none() {
        rates_unset.push("costs.currency".into());
    }
    // Preview accounting: transcript characters/4 + 256 tokens/frame;
    // windowed extraction reads the timeline once and its summaries once.
    let transcript_tokens = u64::try_from(source_body(&source.markdown).chars().count())
        .unwrap_or(0)
        .div_ceil(4);
    let projected_audio_tokens = if transcribe_audio {
        analysis.duration_ms.div_ceil(60_000) * 200
    } else {
        0
    };
    let frame_tokens = u64::try_from(frames.len()).unwrap_or(0) * 256;
    let text_tokens = (transcript_tokens + projected_audio_tokens + frame_tokens).max(1)
        * if counts.extract > 1 { 2 } else { 1 };
    let audio_minutes = if transcribe_audio {
        u32::try_from(analysis.duration_ms.div_ceil(1000)).unwrap_or(0)
    } else {
        0
    };
    let amount = f64::from(audio_minutes) / 60.0 * audio_rate
        + f64::from(u32::try_from(frames.len()).unwrap_or(0)) * image_rate
        + f64::from(u32::try_from(text_tokens).unwrap_or(u32::MAX)) / 1000.0 * text_rate;
    if !amount.is_finite() {
        return Err(refusal(
            "invalid_video_plan",
            "cost estimate exceeds numeric range",
        ));
    }
    let estimate = if rates_unset.is_empty() {
        json!({"currency":settings.costs.currency,"amount":amount,"text_tokens":text_tokens})
    } else {
        Value::Null
    };
    let mut plan = VideoPlan {
        digest: String::new(),
        id: meta.id,
        original_sha256: sha256_hex(&source.bytes),
        transcript_sha256: sha256_hex(source_body(&source.markdown).as_bytes()),
        settings_digest: settings_digest(settings)?,
        sensitivity: sensitivity.into(),
        duration_ms: analysis.duration_ms,
        transcribe_audio,
        frames,
        counts,
        routes: json!({"audio":route,"frames":route,"extract":route}),
        estimate,
        rates_unset,
    };
    plan.digest = digest(&plan)?;
    Ok(plan)
}
fn digest(plan: &VideoPlan) -> Result<String> {
    let mut encoded = serde_json::to_value(plan)
        .map_err(|_| refusal("invalid_video_plan", "cannot encode preview"))?;
    encoded["digest"] = json!("");
    Ok(sha256_hex(&serde_json::to_vec(&encoded).map_err(|_| {
        refusal("invalid_video_plan", "cannot encode preview")
    })?))
}
/// Verify content and policy before consuming any approved budget.
pub fn verify(source: &VideoSource, settings: &Settings, state: &VideoState) -> Result<()> {
    let plan = &state.plan;
    let meta = parse_source_meta(&source.markdown);
    if plan.digest != digest(plan)?
        || plan.original_sha256 != sha256_hex(&source.bytes)
        || state
            .transcription_sha256
            .as_deref()
            .unwrap_or(&plan.transcript_sha256)
            != sha256_hex(source_body(&source.markdown).as_bytes())
        || plan.settings_digest != settings_digest(settings)?
        || plan.id != meta.id
        || plan.sensitivity != meta.sensitivity
    {
        return Err(refusal(
            "plan_changed",
            "video content, policy or route changed; preview and confirm again",
        ));
    }
    Ok(())
}
/// Apply a mutation after the adapter checks the source revision under its writer lock.
pub fn apply(
    source: &VideoSource,
    settings: &Settings,
    mutation: VideoMutation,
) -> Result<VideoWrite> {
    let previous = state(&source.markdown)?;
    let mut files = Vec::new();
    let next = match mutation {
        VideoMutation::ReserveProcessing {
            digest,
            group,
            index,
        } => {
            let mut old = approved(source, settings, previous, &digest)?;
            let (used, cap) = match group.as_str() {
                "audio" => (&mut old.calls.audio, old.plan.counts.audio),
                "extract" => (&mut old.calls.extract, old.plan.counts.extract),
                _ => {
                    return Err(refusal(
                        "invalid_video_plan",
                        "unknown processing call group",
                    ));
                }
            };
            if *used != index || index >= cap {
                return Err(refusal(
                    "plan_exceeded",
                    "processing call would exceed the confirmed plan or repeat an uncertain call",
                ));
            }
            *used += 1;
            old
        }
        VideoMutation::Plan {
            plan,
            confirm_digest,
            at,
        } => {
            if plan.settings_digest != settings_digest(settings)?
                || plan.original_sha256 != sha256_hex(&source.bytes)
                || plan.digest != digest(&plan)?
                || plan.id != super::parse_source_meta(&source.markdown).id
                || plan.transcript_sha256 != sha256_hex(source_body(&source.markdown).as_bytes())
            {
                return Err(refusal(
                    "plan_changed",
                    "video inputs changed during preview",
                ));
            }
            if let Some(digest) = confirm_digest {
                let old = previous
                    .ok_or_else(|| refusal("plan_changed", "preview the plan before confirming"))?;
                if old.plan.digest != digest || plan.digest != digest {
                    return Err(refusal(
                        "plan_changed",
                        "preview changed; confirm its current digest",
                    ));
                }
                let mut old = old;
                if old.confirmed_at.is_none() {
                    old.confirmed_at = Some(at);
                    if old.plan.frames.is_empty() {
                        files.push(("frames.md".into(), Vec::new()));
                    }
                }
                old
            } else if let Some(old) = previous.filter(|old| old.plan.digest == plan.digest) {
                old
            } else {
                VideoState {
                    audio_completed: 0,
                    transcription_sha256: None,
                    plan: *plan,
                    confirmed_at: None,
                    calls: VideoCalls::default(),
                    frames_read: 0,
                    readings: Vec::new(),
                }
            }
        }
        VideoMutation::Reserve { digest, index } => {
            let mut old = approved(source, settings, previous, &digest)?;
            if old.calls.frames != index
                || index >= old.plan.counts.frames
                || index >= old.plan.frames.len()
                || old.frames_read != index
            {
                return Err(refusal(
                    "plan_exceeded",
                    "frame call would exceed the confirmed plan or repeat an uncertain call",
                ));
            }
            old.calls.frames += 1;
            old
        }
        VideoMutation::Frame {
            digest,
            index,
            image,
            text,
            derived_by,
        } => {
            let mut old = approved(source, settings, previous, &digest)?;
            if index >= old.plan.frames.len()
                || old.calls.frames != index + 1
                || old.frames_read != index
            {
                return Err(refusal(
                    "plan_exceeded",
                    "frame has no outstanding planned reservation",
                ));
            }
            let route = derived_by.route.to_string();
            if old.plan.sensitivity == "private"
                && (old.plan.routes["frames"]["route"] != route
                    || old.plan.routes["frames"]["model"] != derived_by.model)
            {
                return Err(refusal(
                    "plan_changed",
                    "frame model route differs from the approved private route",
                ));
            }
            old.readings.push(FrameReading {
                index,
                text,
                route,
                model: derived_by.model,
            });
            old.frames_read += 1;
            files.push((format!("frames/{index:04}.png"), image));
            let body = old
                .readings
                .iter()
                .map(|reading| {
                    let seconds = old.plan.frames[reading.index].time_ms / 1000;
                    let stamp = format!(
                        "{:02}:{:02}:{:02}",
                        seconds / 3600,
                        (seconds % 3600) / 60,
                        seconds % 60
                    );
                    format!(
                        "[{stamp}] {} [frame](#frame={stamp})",
                        reading
                            .text
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            files.push(("frames.md".into(), format!("{body}\n").into_bytes()));
            old
        }
    };
    let (header, body) = source
        .markdown
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---"))
        .ok_or_else(|| refusal("invalid_video_source", "video source needs frontmatter"))?;
    let mut header = header
        .lines()
        .filter(|line| !line.starts_with("video:") && !line.starts_with("sensitivity:"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    // Preserve every existing provenance field, replacing frames once.
    if let Some(reading) = next.readings.last() {
        let filtered = header
            .iter()
            .filter(|line| !line.starts_with("video:"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let fields = crate::domain::knowledge::frontmatter::parse(&filtered)?;
        let mut derived = fields
            .into_iter()
            .find(|(key, _)| key == "derived_by")
            .map(|(_, value)| value)
            .unwrap_or_else(|| json!({}));
        let map = derived
            .as_object_mut()
            .ok_or_else(|| refusal("invalid_video_source", "derived_by must be a mapping"))?;
        map.insert(
            "frames".into(),
            json!({"route":reading.route,"model":reading.model}),
        );
        header.retain(|line| !line.starts_with("derived_by:"));
        header.push(format!("derived_by: {}", flow(&derived)));
    }
    header.push(format!("sensitivity: {}", next.plan.sensitivity));
    header.push(format!("video: {}", encode_state(&next)?));
    Ok(VideoWrite {
        markdown: format!("---\n{}\n---{body}", header.join("\n")),
        files,
        state: next,
    })
}
fn approved(
    source: &VideoSource,
    settings: &Settings,
    previous: Option<VideoState>,
    digest: &str,
) -> Result<VideoState> {
    let old = previous
        .filter(|old| old.confirmed_at.is_some())
        .ok_or_else(|| {
            refusal(
                "plan_unconfirmed",
                "preview and confirm the video plan first",
            )
        })?;
    verify(source, settings, &old)?;
    if old.plan.digest != digest {
        return Err(refusal("plan_changed", "confirmed digest changed"));
    }
    Ok(old)
}
