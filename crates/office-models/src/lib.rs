//! Model adapters for the office plugin, behind `office-core` ports.
//!
//! The only crate allowed an HTTP client (ARCHITECTURE.md). Calls are
//! blocking: the plugin backend is one process per call. Secrets arrive
//! from the call context and never appear in errors or logs (STD-05).
//!
//! # Confirmed request and response shapes (2026-10-27)
//!
//! Spec §18.5 left these shapes unverified; they were confirmed against
//! the providers' published documentation before this implementation:
//!
//! - **Meta Model API** (<https://api.meta.ai/v1>): Bearer credential,
//!   OpenAI-compatible Chat Completions, Responses and Messages formats
//!   (LiteLLM's Meta provider page and Meta-derived developer guides,
//!   fetched 2026-10-27). Hosted extraction and vision therefore use
//!   `POST /v1/chat/completions` with model `muse-spark-1.3`; the reply
//!   text is `choices[0].message.content`. No dedicated Muse Voice
//!   Transcribe request shape was published in the fetched references,
//!   so transcription sends the audio as a base64 `input_audio` content
//!   part through the same confirmed shape and takes the reply as the
//!   transcript. This assumption is the one unverified remainder.
//! - **Baseten Model APIs** (<https://docs.baseten.co/inference/model-apis/overview>,
//!   fetched 2026-10-27): Chat Completions at
//!   `https://inference.baseten.co/v1/chat/completions`, Responses at
//!   `/v1/responses`, Bearer credential, deployment endpoints under
//!   `https://model-<id>.api.baseten.co/...`. Muse Spark Open
//!   (`meta-models/muse-spark-open-1026`) accepts `json_schema` but does
//!   not enforce it, so open-weights extraction parses and refuses
//!   non-JSON replies instead of trusting the shape.
//!
//! Only the Meta and Baseten hosts are reachable; any other host is
//! refused in code ([`http::check_url`]). Every call crosses the one
//! routing function ([`routing::route`]): `private` material goes only
//! to the open-weights endpoint from Library settings and is refused
//! when none is configured.
//!
//! Transcription takes its duration from `TranscribeRequest::duration_seconds`
//! (caller-supplied metadata, rounded up); this adapter never probes media.
//! `ModelConfig::audio_splitter` defaults to splitting only above 600 seconds,
//! into chunks of at most 600 seconds. Short recordings go straight to the
//! routed call without files or subprocesses. Long recordings are staged in a
//! private temporary directory and split by supervised ffmpeg. Each chunk uses
//! the original sensitivity, MIME type and language hint. Plain-text replies
//! are joined with newlines in recording order; speaker/timestamp segments stay
//! empty because the provider currently returns only text. A failed chunk fails
//! the transcription; temporary audio is removed on success and error.
//! Long audio supports WAV, MP3, M4A/MP4, AAC, FLAC, Ogg and WebM MIME types;
//! other containers are refused before staging rather than given a false suffix.

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod baseten;
mod http;
pub mod media;
mod meta;
mod routing;
mod transcribe;

use std::time::Duration;

use office_core::Error;
use office_core::Result;
use office_core::ports::{
    ExtractRequest, ImageReading, ImageRequest, Models, TranscribeRequest, Transcript,
};
use serde_json::Value;

/// Default hosted model (Muse Spark).
pub const DEFAULT_HOSTED_MODEL: &str = "muse-spark-1.3";
/// Default open-weights model (Muse Spark Open on Baseten).
pub const DEFAULT_OPEN_WEIGHTS_MODEL: &str = "meta-models/muse-spark-open-1026";
/// Default Meta Model API base URL.
pub const DEFAULT_META_BASE_URL: &str = "https://api.meta.ai/v1";

/// Endpoints, credentials and limits for [`RoutedClient`].
///
/// Credentials come from the call context (`meta_api_key`,
/// `baseten_token`); the open-weights endpoint comes from Library
/// settings. No field's value is ever logged or put in an error.
#[derive(Clone, Debug, Default)]
pub struct ModelConfig {
    /// Meta Model API key (hosted calls). Required for `normal` material.
    pub meta_api_key: Option<String>,
    /// Baseten token (open-weights calls), when the endpoint needs one.
    pub baseten_token: Option<String>,
    /// Hosted Meta base URL.
    pub meta_base_url: String,
    /// Open-weights base URL from Library settings; `None` refuses `private`.
    pub open_weights_endpoint: Option<String>,
    /// Hosted model slug.
    pub hosted_model: String,
    /// Open-weights model slug.
    pub open_weights_model: String,
    /// Executable, split threshold and maximum chunk length for transcription.
    pub audio_splitter: media::AudioSplitter,
}

impl ModelConfig {
    /// Configuration with the published default endpoints and models.
    #[must_use]
    pub fn new(meta_api_key: Option<String>, baseten_token: Option<String>) -> Self {
        Self {
            meta_api_key,
            baseten_token,
            meta_base_url: DEFAULT_META_BASE_URL.to_string(),
            open_weights_endpoint: None,
            hosted_model: DEFAULT_HOSTED_MODEL.to_string(),
            open_weights_model: DEFAULT_OPEN_WEIGHTS_MODEL.to_string(),
            audio_splitter: media::AudioSplitter::default(),
        }
    }
}

/// The routed model client: implements the `office-core` model port over
/// the Meta and Baseten endpoints, routing every call by sensitivity.
pub struct RoutedClient {
    config: ModelConfig,
    agent: ureq::Agent,
}

impl RoutedClient {
    /// Build a client. Endpoint hosts are allowlist-checked on every
    /// call, before any socket opens, not only here.
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self { config, agent }
    }

    fn call(
        &self,
        sensitivity: office_common::Sensitivity,
        operation: Operation<'_>,
    ) -> Result<String> {
        let resolved = routing::route(sensitivity, &self.config)?;
        let url = meta::chat_url(&resolved.base_url);
        let host = http::host_of(&url).unwrap_or_default();
        let body = match operation {
            Operation::Transcribe {
                mime,
                audio,
                language,
            } => meta::transcribe_body(&resolved.model, mime, audio, language),
            Operation::Extract { text, schema } => {
                if resolved.open_weights {
                    baseten::extract_body(&resolved.model, text, schema)
                } else {
                    meta::extract_body(&resolved.model, text, schema)
                }
            }
            Operation::Image {
                mime,
                image,
                prompt,
            } => meta::image_body(&resolved.model, mime, image, prompt),
        };
        let response = http::post_json(&self.agent, &url, resolved.api_key.as_deref(), &body)?;
        meta::reply_text(&host, &response)
    }
}

enum Operation<'a> {
    Transcribe {
        mime: &'a str,
        audio: &'a [u8],
        language: Option<&'a str>,
    },
    Extract {
        text: &'a str,
        schema: &'a Value,
    },
    Image {
        mime: &'a str,
        image: &'a [u8],
        prompt: &'a str,
    },
}

impl Models for RoutedClient {
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Transcript> {
        transcribe::transcribe(self, request)
    }

    fn extract(&self, request: &ExtractRequest) -> Result<Value> {
        let text = self.call(
            request.sensitivity,
            Operation::Extract {
                text: &request.text,
                schema: &request.schema,
            },
        )?;
        serde_json::from_str(&text).map_err(|_| Error::Model {
            code: "model_invalid_response",
            message: "model extraction reply was not JSON".to_string(),
            retryable: false,
        })
    }

    fn read_image(&self, request: &ImageRequest) -> Result<ImageReading> {
        let text = self.call(
            request.sensitivity,
            Operation::Image {
                mime: &request.mime,
                image: &request.image,
                prompt: &request.prompt,
            },
        )?;
        let fields = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(ImageReading { text, fields })
    }
}

#[cfg(test)]
mod tests;
