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
//!   part through the same confirmed shape. The reply is parsed as JSON
//!   `{"text","segments"}` when the endpoint reports that object (a
//!   markdown code fence around the object is accepted); each segment is
//!   a speaker turn with millisecond `start_ms` and `end_ms`. A reply
//!   that is not that JSON is kept as the transcript text and its
//!   segments stay empty. The live model's compliance with the requested
//!   JSON is still unverified.
//! - **The private route** (Muse Spark Open on the endpoint the user
//!   chose: a company's vLLM or SGLang deployment, a hosted provider such
//!   as Baseten's Model APIs, <https://docs.baseten.co/inference/model-apis/overview>,
//!   fetched 2026-10-27, or the user's own hardware): OpenAI-compatible
//!   Chat Completions at `<endpoint>/chat/completions`, optional Bearer
//!   credential (`open_weights_token`), and the model named as the server
//!   names it (`open_weights.model`). Muse Spark Open accepts `json_schema`
//!   but does not enforce it. Extraction on both routes parses the reply as
//!   JSON and validates it locally against the requested schema. Non-JSON or
//!   schema-violating replies fail with non-retryable `model_invalid_response`.
//!   Invalid schemas or external references fail before inference with
//!   non-retryable `model_invalid_schema`; local references are supported,
//!   and schema validation never retrieves URLs or files.
//!
//! [`http::check_url`] allows `api.meta.ai` for hosted calls and, for
//! private calls, only the configured endpoint's exact origin (HTTPS unless
//! loopback, never `api.meta.ai`), refusing every other host before a
//! socket opens. Every call crosses the one routing function
//! ([`routing::route`]): `private` material, at every tier and modality,
//! goes only to the configured model, and is refused when no endpoint is
//! configured, when the endpoint has not passed
//! `office.private-route-check`'s text request, or when it lacks the
//! call's modality (`private_media_unsupported`). Every result carries
//! its `derived_by` route (`hosted` or `private:<kind>`).
//! [`Models::check_private_route`] sends the check's three tiny requests
//! to the configured endpoint regardless of its recorded capabilities.
//!
//! Transcription takes its duration from `TranscribeRequest::duration_seconds`
//! (caller-supplied metadata, rounded up); this adapter never probes media.
//! `ModelConfig::audio_splitter` defaults to splitting only above 600 seconds,
//! into chunks of at most 600 seconds. Short recordings go straight to the
//! routed call without files or subprocesses. Long recordings are staged in a
//! private temporary directory and split by supervised ffmpeg. Each chunk uses
//! the original sensitivity, MIME type and language hint. Each chunk's
//! transcript text is joined with newlines in recording order. Speaker
//! turns parsed from a chunk are kept, and their timestamps are shifted
//! by that chunk's start so they stay offsets into the original recording.
//! A chunk whose reply is not the JSON transcript shape, or whose `segments`
//! are empty, contributes text only. Those chunks stay in recording order
//! beside the turns, so the meeting body can keep a text-only chunk in the
//! position it occupies. A failed chunk fails the transcription; temporary
//! audio is removed on success and error.
//! Transcription accepts WAV (`audio/wav`, `audio/wave`, `audio/x-wav`) and
//! MP3 (`audio/mpeg`, `audio/mp3`) audio, sent to the model as `wav` or `mp3`.
//! Other containers are refused before staging or any model call, because the
//! model schema takes no other `input_audio` format.

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod http;
pub mod media;
mod meta;
mod open_weights;
mod route_check;
mod routing;
mod transcribe;

use std::time::Duration;

use office_core::Error;
use office_core::Result;
use office_core::domain::endpoint::Capability;
use office_core::domain::settings::OpenWeights;
use office_core::ports::{
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, RouteCheck, Routed,
    TranscribeRequest, Transcript,
};
use serde_json::Value;

/// Default hosted model (Muse Spark).
pub const DEFAULT_HOSTED_MODEL: &str = "muse-spark-1.3";
/// Default Meta Model API base URL.
pub const DEFAULT_META_BASE_URL: &str = "https://api.meta.ai/v1";

/// Endpoints, credentials and limits for [`RoutedClient`].
///
/// Credentials come from the call context (`meta_api_key`,
/// `open_weights_token`); the private endpoint comes from Library
/// settings. No credential is ever logged or put in an error.
#[derive(Clone, Debug, Default)]
pub struct ModelConfig {
    /// Meta Model API key (hosted calls). Required for `normal` material.
    pub meta_api_key: Option<String>,
    /// The private endpoint's token, when it needs one.
    pub open_weights_token: Option<String>,
    /// Hosted Meta base URL.
    pub meta_base_url: String,
    /// The private route from Library settings; `None` refuses `private`.
    pub open_weights: Option<OpenWeights>,
    /// Hosted model slug.
    pub hosted_model: String,
    /// Executable, split threshold and maximum chunk length for transcription.
    pub audio_splitter: media::AudioSplitter,
}

impl ModelConfig {
    /// Configuration with the published hosted endpoint and model, and no
    /// private route.
    #[must_use]
    pub fn new(meta_api_key: Option<String>, open_weights_token: Option<String>) -> Self {
        Self {
            meta_api_key,
            open_weights_token,
            meta_base_url: DEFAULT_META_BASE_URL.to_string(),
            open_weights: None,
            hosted_model: DEFAULT_HOSTED_MODEL.to_string(),
            audio_splitter: media::AudioSplitter::default(),
        }
    }
}

/// The routed model client: implements the `office-core` model port over
/// the hosted Meta API and the private endpoint, routing every call by
/// sensitivity.
pub struct RoutedClient {
    config: ModelConfig,
    agent: ureq::Agent,
}

impl RoutedClient {
    /// Build a client. Endpoint hosts are allowlist-checked on every
    /// call, before any socket opens, not only here. Redirects are disabled
    /// so a response cannot send a later request to an unchecked host.
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .build()
            .into();
        Self { config, agent }
    }

    /// Route one call needing `needs` and post its JSON body. Every model
    /// operation crosses this function, so endpoint selection stays in
    /// [`routing::route`]. Returns the host, the response and the route.
    fn post(
        &self,
        sensitivity: office_common::Sensitivity,
        needs: Capability,
        body_for: impl FnOnce(&routing::ResolvedCall) -> Value,
    ) -> Result<(String, Value, DerivedBy)> {
        let resolved = routing::route(sensitivity, needs, &self.config)?;
        let (host, response) = self.send(&resolved, &body_for(&resolved))?;
        Ok((host, response, resolved.derived_by()))
    }

    /// Post `body` to a resolved endpoint, allowlist-checked first.
    fn send(&self, resolved: &routing::ResolvedCall, body: &Value) -> Result<(String, Value)> {
        self.send_with_timeout(resolved, body, None)
    }

    /// Post `body` with a per-request timeout overriding the agent's cap.
    fn send_with_timeout(
        &self,
        resolved: &routing::ResolvedCall,
        body: &Value,
        timeout: Option<Duration>,
    ) -> Result<(String, Value)> {
        let url = meta::chat_url(&resolved.base_url);
        let host = http::host_of(&url).unwrap_or_default();
        let response = http::post_json(
            &self.agent,
            &url,
            resolved.api_key.as_deref(),
            body,
            resolved.destination(),
            timeout,
        )?;
        Ok((host, response))
    }

    fn call(
        &self,
        sensitivity: office_common::Sensitivity,
        operation: Operation<'_>,
    ) -> Result<(String, DerivedBy)> {
        let needs = match operation {
            Operation::Extract { .. } => Capability::Text,
            Operation::Image { .. } => Capability::Image,
        };
        let (host, response, derived_by) =
            self.post(sensitivity, needs, |resolved| match operation {
                Operation::Extract { text, schema } => {
                    if resolved.is_private() {
                        open_weights::extract_body(&resolved.model, text, schema)
                    } else {
                        meta::extract_body(&resolved.model, text, schema)
                    }
                }
                Operation::Image {
                    mime,
                    image,
                    prompt,
                } => meta::image_body(&resolved.model, mime, image, prompt),
            })?;
        Ok((meta::reply_text(&host, &response)?, derived_by))
    }

    /// Transcribe one audio chunk in `format` (`wav` or `mp3`). Segment
    /// timestamps are relative to this chunk; the caller shifts them onto the
    /// original recording.
    fn transcribe_chunk(
        &self,
        sensitivity: office_common::Sensitivity,
        format: &str,
        audio: &[u8],
        language: Option<&str>,
    ) -> Result<meta::ParsedTranscript> {
        let (host, response, _) = self.post(sensitivity, Capability::Audio, |resolved| {
            meta::transcribe_body(&resolved.model, format, audio, language)
        })?;
        meta::parse_transcript(&host, &response)
    }
}

enum Operation<'a> {
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
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Routed<Transcript>> {
        transcribe::transcribe(self, request)
    }

    fn extract(&self, request: &ExtractRequest) -> Result<Routed<Value>> {
        // Compile before inference, failing closed without retrieving external
        // resources (jsonschema's HTTP and file features are disabled).
        // Schema errors and validation errors may contain private input, so
        // neither is included in diagnostics.
        let validator = jsonschema::validator_for(&request.schema).map_err(|_| Error::Model {
            code: "model_invalid_schema",
            message: "request.schema could not be compiled as a self-contained JSON Schema"
                .to_string(),
            retryable: false,
        })?;
        let (text, derived_by) = self.call(
            request.sensitivity,
            Operation::Extract {
                text: &request.text,
                schema: &request.schema,
            },
        )?;
        let value = serde_json::from_str(&text).map_err(|_| Error::Model {
            code: "model_invalid_response",
            message: "model extraction reply was not JSON".to_string(),
            retryable: false,
        })?;
        if !validator.is_valid(&value) {
            return Err(Error::Model {
                code: "model_invalid_response",
                message: "model extraction reply did not satisfy request.schema".to_string(),
                retryable: false,
            });
        }
        Ok(Routed { value, derived_by })
    }

    fn read_image(&self, request: &ImageRequest) -> Result<Routed<ImageReading>> {
        let (text, derived_by) = self.call(
            request.sensitivity,
            Operation::Image {
                mime: &request.mime,
                image: &request.image,
                prompt: &request.prompt,
            },
        )?;
        let fields = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(Routed {
            value: ImageReading { text, fields },
            derived_by,
        })
    }

    fn check_private_route(&self) -> Result<RouteCheck> {
        route_check::check(self)
    }
}

#[cfg(test)]
mod tests;
