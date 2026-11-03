//! Model adapters for the office plugin, behind `office-core` ports.
//!
//! The only crate allowed an HTTP client (ARCHITECTURE.md). Calls are
//! blocking: the plugin backend is one process per call. Secrets arrive
//! from the call context and never appear in errors or logs (STD-05).
//!
//! # Confirmed request and response shapes (2026-10-27)
//!
//! These are the default provider's shapes. Another hosted endpoint gets
//! the same OpenAI-compatible Chat Completions bodies, and its audio as a
//! Chat Completions `input_audio` part rather than Meta's ASR endpoint.
//!
//! Spec §18.5 left these shapes unverified; they were confirmed against
//! the providers' published documentation before this implementation:
//!
//! - **Meta Model API** (<https://api.meta.ai/v1>): Bearer credential,
//!   OpenAI-compatible Chat Completions, Responses and Messages formats
//!   (LiteLLM's Meta provider page and Meta-derived developer guides,
//!   fetched 2026-10-27). Hosted extraction and vision therefore use
//!   `POST /v1/chat/completions` with model `muse-spark-1.3`; the reply
//!   text is `choices[0].message.content`. Hosted extraction sends its
//!   schema with `strict: false`: live on 2026-11-02 the API refused a
//!   strict schema with an optional property (HTTP 400, "'required' is
//!   required to be supplied and to be an array including every key in
//!   properties"), and the reply is validated locally either way.
//!   Hosted transcription uses Muse Voice Transcribe
//!   (`muse-voice-transcribe-1.0`, listed by `GET /v1/models` on
//!   2026-11-02) through its one-shot endpoint `POST /v1/asr/transcribe`
//!   (Meta's `meta-model-cookbook`, `06_muse_voice`, fetched 2026-11-02):
//!   multipart parts `request` (JSON: `model`, `mode: "DIARIZATION"`,
//!   `audioEncoding: "WAV"`, optional `languageBias`) and `audio` (the WAV).
//!   The reply's `transcript` is the text, and each `turns` entry with
//!   `startMs` and `transcript` becomes a segment, ending at its `endMs`,
//!   else the next turn's start, else the end of the audio. Chat
//!   Completions does not serve that model (HTTP 404 `model_not_found`
//!   live). MP3 audio is decoded locally to WAV first.
//! - **The private route** (Muse Spark Open on the endpoint the user
//!   chose: a company's vLLM or SGLang deployment, a hosted provider such
//!   as Baseten's Model APIs, <https://docs.baseten.co/inference/model-apis/overview>,
//!   fetched 2026-10-27, or the user's own hardware): OpenAI-compatible
//!   Chat Completions at `<endpoint>/chat/completions`, optional Bearer
//!   credential (`open_weights_token`, sent only to the origin it was
//!   saved for), and the model named as the server
//!   names it (`open_weights.model`). Muse Spark Open accepts `json_schema`
//!   but does not enforce it. Extraction on both routes parses the reply as
//!   JSON and validates it locally against the requested schema. Non-JSON or
//!   schema-violating replies fail with non-retryable `model_invalid_response`.
//!   Invalid schemas or external references fail before inference with
//!   non-retryable `model_invalid_schema`; local references are supported,
//!   and schema validation never retrieves URLs or files.
//!
//! The hosted endpoint is configurable (`models.endpoint`, any
//! OpenAI-compatible provider; Meta's Model API while unset), and so are
//! its models (`models.standard`, `models.careful`, `models.transcribe`).
//! [`http::check_url`] allows, for hosted calls, only the configured
//! hosted endpoint's exact origin and, for private calls, only the
//! configured private endpoint's exact origin (never `api.meta.ai`); both
//! over HTTPS unless loopback, refusing every other host before a socket
//! opens. Every call crosses the one routing function
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
//! MP3 (`audio/mpeg`, `audio/mp3`) audio. The private route receives it as
//! `wav` or `mp3` `input_audio`; the hosted route always receives WAV.
//! Other containers are refused before staging or any model call; callers
//! decode them first (`Models::media_audio`, which `office.transcribe` uses
//! for every meeting and video original, M4A and MP4 included).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod chat;
mod http;
pub mod media;
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
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, ModelTier, Models, RouteCheck, Routed,
    TranscribeRequest, Transcript,
};
use serde_json::Value;

/// Default hosted model for the standard tier (Muse Spark).
pub const DEFAULT_HOSTED_MODEL: &str = office_core::domain::settings::DEFAULT_STANDARD_MODEL;
/// Default hosted base URL: Meta's Model API, used while `models.endpoint`
/// is unset.
pub const DEFAULT_HOSTED_BASE_URL: &str = office_core::domain::endpoint::DEFAULT_HOSTED_ENDPOINT;

/// Endpoints, credentials and limits for [`RoutedClient`].
///
/// Credentials come from the call context (`meta_api_key` for the hosted
/// endpoint, `open_weights_token`); both endpoints come from Library
/// settings. No credential is ever logged or put in an error.
#[derive(Clone, Debug, Default)]
pub struct ModelConfig {
    /// The hosted endpoint's API key (the `meta_api_key` secret, whatever
    /// provider `models.endpoint` names). Required for `normal` material.
    pub hosted_api_key: Option<String>,
    /// The stored `open_weights_token` record, exactly as Orbit delivered
    /// it: the private endpoint's token bound to the origin it was saved
    /// for ([`office_core::domain::endpoint::BoundToken`]). Routing sends
    /// the token only to that origin; any other value is never sent.
    pub open_weights_token: Option<String>,
    /// Hosted OpenAI-compatible base URL (`models.endpoint`); every hosted
    /// call must open a socket to exactly its origin.
    pub hosted_base_url: String,
    /// Whether hosted audio uses Meta's one-shot `/asr/transcribe`
    /// endpoint (true for Meta's Model API). Otherwise hosted audio goes
    /// to Chat Completions as `input_audio`, as on the private route.
    pub hosted_asr: bool,
    /// The private route from Library settings; `None` refuses `private`.
    pub open_weights: Option<OpenWeights>,
    /// Hosted model slug.
    pub hosted_model: String,
    /// Hosted model for careful synthesis; defaults to Muse Wonder.
    pub careful_model: String,
    /// Hosted transcription model; defaults to Muse Voice Transcribe.
    pub transcribe_model: String,
    /// Executable, split threshold and maximum chunk length for transcription.
    pub audio_splitter: media::AudioSplitter,
}

impl ModelConfig {
    /// Configuration with the default hosted endpoint (Meta's Model API)
    /// and models, and no private route.
    #[must_use]
    pub fn new(hosted_api_key: Option<String>, open_weights_token: Option<String>) -> Self {
        Self::with_models(
            hosted_api_key,
            open_weights_token,
            &office_core::domain::settings::ModelSettings::default(),
        )
    }

    /// Configuration with the hosted endpoint and models from Library
    /// settings (`models.*`), and no private route.
    #[must_use]
    pub fn with_models(
        hosted_api_key: Option<String>,
        open_weights_token: Option<String>,
        models: &office_core::domain::settings::ModelSettings,
    ) -> Self {
        Self {
            hosted_api_key,
            open_weights_token,
            hosted_base_url: models.hosted_endpoint().to_string(),
            hosted_asr: models.hosted_is_meta(),
            open_weights: None,
            hosted_model: models.standard_model().to_string(),
            careful_model: models.careful.clone(),
            transcribe_model: models.transcribe_model().to_string(),
            audio_splitter: media::AudioSplitter::default(),
        }
    }
}

/// The routed model client: implements the `office-core` model port over
/// the configured hosted endpoint and the private endpoint, routing every call by
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
        tier: ModelTier,
        body_for: impl FnOnce(&routing::ResolvedCall) -> Value,
    ) -> Result<(String, Value, DerivedBy)> {
        let resolved = routing::route(sensitivity, needs, tier, &self.config)?;
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
        let url = chat::chat_url(&resolved.base_url);
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
        let tier = match operation {
            Operation::Extract { tier, .. } => tier,
            Operation::Image { .. } => ModelTier::Standard,
        };
        let needs = match operation {
            Operation::Extract { .. } => Capability::Text,
            Operation::Image { .. } => Capability::Image,
        };
        let (host, response, derived_by) =
            self.post(sensitivity, needs, tier, |resolved| match operation {
                Operation::Extract { text, schema, .. } => {
                    if resolved.is_private() {
                        open_weights::extract_body(&resolved.model, text, schema)
                    } else {
                        chat::extract_body(&resolved.model, text, schema)
                    }
                }
                Operation::Image {
                    mime,
                    image,
                    prompt,
                } => chat::image_body(&resolved.model, mime, image, prompt),
            })?;
        Ok((chat::reply_text(&host, &response)?, derived_by))
    }

    /// Transcribe one audio chunk of `seconds` in `format` (`wav` or `mp3`).
    /// Hosted chunks on Meta's Model API go to Muse Voice Transcribe as WAV,
    /// an MP3 chunk being decoded locally first; private chunks, and hosted
    /// chunks on any other configured endpoint, keep the chat `input_audio`
    /// shape. Segment timestamps are relative to this chunk; the caller
    /// shifts them onto the original recording.
    fn transcribe_chunk(
        &self,
        sensitivity: office_common::Sensitivity,
        format: &str,
        audio: &[u8],
        seconds: u64,
        language: Option<&str>,
    ) -> Result<chat::ParsedTranscript> {
        let resolved = routing::route(
            sensitivity,
            Capability::Audio,
            ModelTier::Standard,
            &self.config,
        )?;
        if resolved.is_private() || !self.config.hosted_asr {
            let body = chat::transcribe_body(&resolved.model, format, audio, language);
            let (host, response) = self.send(&resolved, &body)?;
            return chat::parse_transcript(&host, &response);
        }
        let decoded;
        let wav = if format == "wav" {
            audio
        } else {
            decoded = media::VideoProcessor {
                ffmpeg: self.config.audio_splitter.ffmpeg_path.clone(),
                ..media::VideoProcessor::default()
            }
            .audio(audio, 0, seconds)?;
            decoded.as_slice()
        };
        let url = transcription_url(&resolved, self.config.hosted_asr);
        let host = http::host_of(&url).unwrap_or_default();
        let (content_type, body) = chat::asr_body(&resolved.model, wav, language);
        let response = http::post_bytes(
            &self.agent,
            &url,
            resolved.api_key.as_deref(),
            &content_type,
            &body,
            resolved.destination(),
            None,
        )?;
        chat::parse_asr(&host, &response)
    }
}

/// Where a resolved transcription call is sent: Muse Voice Transcribe when
/// hosted on Meta's Model API, else Chat Completions.
fn transcription_url(resolved: &routing::ResolvedCall, hosted_asr: bool) -> String {
    if resolved.is_private() || !hosted_asr {
        chat::chat_url(&resolved.base_url)
    } else {
        chat::asr_url(&resolved.base_url)
    }
}

enum Operation<'a> {
    Extract {
        tier: ModelTier,
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
    fn media_audio(&self, bytes: &[u8], start: u64, length: u64) -> Result<Vec<u8>> {
        media::VideoProcessor {
            ffmpeg: self.config.audio_splitter.ffmpeg_path.clone(),
            ..media::VideoProcessor::default()
        }
        .audio(bytes, start, length)
    }
    fn transcribe_planned(
        &self,
        request: &TranscribeRequest,
        _settings_digest: &str,
    ) -> Result<Routed<Transcript>> {
        transcribe::planned(self, request)
    }
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
                tier: request.tier,
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
