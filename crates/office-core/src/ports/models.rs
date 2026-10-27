//! Model access port.
//!
//! Three operations (specs/models-and-privacy.md): transcribe audio,
//! extract structured JSON from text, and read an image. Every request
//! carries its [`Sensitivity`]; the adapter's one routing function sends
//! `private` material only to the open-weights endpoint (STD-02 R24).
//! Secrets never cross this port: an adapter is constructed with them
//! from the call context by the composition root.

use office_common::Sensitivity;
use serde_json::Value;

use crate::Result;

/// Audio to transcribe, with the sensitivity of its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscribeRequest {
    /// Raw audio bytes (encoding named by `mime`).
    pub audio: Vec<u8>,
    /// MIME type of `audio`, for example `audio/wav`.
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

/// A completed transcription.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Full transcript text.
    pub text: String,
    /// Speaker turns with timestamps, when the model reports them.
    pub segments: Vec<TranscriptSegment>,
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

/// Model access: transcription, structured extraction and image reading.
pub trait Models {
    /// Transcribe audio into text with speaker turns and timestamps.
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Transcript>;
    /// Extract structured JSON, validated against `request.schema`, from text.
    fn extract(&self, request: &ExtractRequest) -> Result<Value>;
    /// Read an image (receipt and bill fields) as text plus structured fields.
    fn read_image(&self, request: &ImageRequest) -> Result<ImageReading>;
}
