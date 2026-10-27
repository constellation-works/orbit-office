//! Model access port.
//!
//! Three operations (specs/models-and-privacy.md): transcribe audio,
//! extract structured JSON from text, and read an image. Every request
//! carries its [`Sensitivity`]; the adapter's one routing function sends
//! `private` material only to the configured private endpoint (STD-02 R24),
//! and every result says where it went ([`Routed::derived_by`]), for the
//! caller to record in `derived_by`. A fourth operation checks the private
//! endpoint for `office.private-route-check`.
//! Secrets never cross this port: an adapter is constructed with them
//! from the call context by the composition root.

use office_common::Sensitivity;
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::domain::endpoint::{Capability, ModelRoute};

/// How a model result was produced: the `derived_by` fields a caller records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DerivedBy {
    /// `hosted` or `private:<kind>`.
    pub route: ModelRoute,
    /// The model named in the request.
    pub model: String,
}

/// A model result with the route that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct Routed<T> {
    /// The result itself.
    pub value: T,
    /// Where the call went and which model answered.
    pub derived_by: DerivedBy,
}

/// One tiny request of the private-endpoint check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteProbe {
    /// The modality this request exercised.
    pub capability: Capability,
    /// Whether the endpoint answered it with a usable reply.
    pub accepted: bool,
    /// Wall time an accepted request took, in milliseconds; `None` when the
    /// request failed (its `error` says why).
    pub elapsed_ms: Option<u64>,
    /// The failure code when it was not accepted (for example
    /// `model_request_rejected`); never a response body.
    pub error: Option<String>,
}

/// What the private-endpoint check measured, and on which endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteCheck {
    /// The endpoint the requests went to (the configured base URL).
    pub endpoint: String,
    /// The model named in the requests (the configured model).
    pub model: String,
    /// The model name the endpoint reported in its first usable reply.
    pub reported_model: Option<String>,
    /// One probe per capability, in check order (text, image, audio).
    pub probes: Vec<RouteProbe>,
}

/// Audio to transcribe, with the sensitivity of its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscribeRequest {
    /// Raw audio bytes (encoding named by `mime`).
    pub audio: Vec<u8>,
    /// Recording duration in seconds, supplied by the caller from media metadata.
    /// Round fractional seconds up so a recording above the split threshold
    /// cannot be treated as short. The adapter does not probe the media.
    pub duration_seconds: u64,
    /// MIME type of `audio`, for example `audio/wav`. Transcription accepts
    /// WAV and MP3 only.
    pub mime: String,
    /// Sensitivity of the source this audio came from.
    pub sensitivity: Sensitivity,
    /// Optional language hint (a language name, for example `English`).
    pub language: Option<String>,
}

/// One speaker turn with timestamps, feeding source anchors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptSegment {
    /// Speaker label when the model reports one.
    pub speaker: Option<String>,
    /// Segment start, milliseconds into the recording.
    pub start_ms: u64,
    /// Segment end, milliseconds into the recording.
    pub end_ms: u64,
    /// Transcribed text of this segment.
    pub text: String,
}

/// One transcribed chunk, in recording order.
///
/// [`Transcript::segments`] lists only speaker turns, so it cannot say where
/// a chunk that reported no turns sat. This enum keeps that chunk's text in
/// place among the turns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptChunk {
    /// Speaker turns from a chunk that reported them. Timestamps are already
    /// offsets into the original recording.
    Turns(Vec<TranscriptSegment>),
    /// Text from a chunk that reported no turns, kept unchanged.
    Text(String),
}

/// A completed transcription.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Full transcript text. Chunk texts are joined with newlines.
    pub text: String,
    /// Speaker turns with timestamps, when the model reports them.
    /// Text-only chunks are omitted; their place is in [`Transcript::chunks`].
    pub segments: Vec<TranscriptSegment>,
    /// Chunks in recording order. Empty when the caller built `text` and
    /// `segments` without chunk boundaries. Transcription of one reply stores
    /// a single chunk; a split recording stores one entry per chunk.
    pub chunks: Vec<TranscriptChunk>,
}

/// Text to extract structured JSON from, with its source's sensitivity.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractRequest {
    /// Source text (the source's model view, never raw contact details).
    pub text: String,
    /// JSON Schema the returned value must satisfy.
    pub schema: Value,
    /// Sensitivity of the source this text came from.
    pub sensitivity: Sensitivity,
}

/// An image to read (receipt and bill field extraction), with sensitivity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRequest {
    /// Raw image bytes (encoding named by `mime`).
    pub image: Vec<u8>,
    /// MIME type of `image`, for example `image/png`.
    pub mime: String,
    /// What to read from the image.
    pub prompt: String,
    /// Sensitivity of the source this image came from.
    pub sensitivity: Sensitivity,
}

/// What the model read from an image.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageReading {
    /// The model's textual reading of the image.
    pub text: String,
    /// Structured fields parsed from the reading, when it was JSON.
    pub fields: Value,
}

/// Model access: transcription, structured extraction and image reading,
/// each with its route, plus the private-endpoint check.
pub trait Models {
    /// Inspect video locally with supervised ffprobe/ffmpeg; spends no model calls.
    fn analyze_video(
        &self,
        _bytes: &[u8],
        _threshold: &serde_json::Number,
    ) -> Result<crate::domain::source::video::VideoAnalysis> {
        Err(crate::domain::source::video::refusal(
            "media_unavailable",
            "video analysis is unavailable",
        ))
    }
    /// Extract one PNG at the planned timestamp, scaled to 640 pixels wide.
    fn video_frame(&self, _bytes: &[u8], _time_ms: u64) -> Result<Vec<u8>> {
        Err(crate::domain::source::video::refusal(
            "media_unavailable",
            "frame extraction is unavailable",
        ))
    }

    /// Transcribe audio into text with speaker turns and timestamps.
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Routed<Transcript>>;
    /// Extract structured JSON, validated against `request.schema`, from text.
    fn extract(&self, request: &ExtractRequest) -> Result<Routed<Value>>;
    /// Read a confirmed video image using exactly the approved policy/endpoint snapshot.
    /// Adapters must compare the digest before constructing the routed client.
    fn read_planned_image(
        &self,
        _request: &ImageRequest,
        _settings_digest: &str,
    ) -> Result<Routed<ImageReading>> {
        Err(crate::domain::source::video::refusal(
            "plan_changed",
            "planned image routing is unavailable",
        ))
    }
    /// Read an image (receipt and bill fields) as text plus structured fields.
    fn read_image(&self, request: &ImageRequest) -> Result<Routed<ImageReading>>;
    /// Send one tiny text, image and audio request to the configured private
    /// endpoint, whatever it was last found to accept, and report each
    /// outcome. Refused when no private endpoint is configured. Records
    /// nothing; the caller stores the result through the Library port.
    fn check_private_route(&self) -> Result<RouteCheck>;
}
