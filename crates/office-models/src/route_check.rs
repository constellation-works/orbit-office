//! The private-endpoint check behind `office.private-route-check`
//! (specs/models-and-privacy.md, "Checking an endpoint").
//!
//! Three tiny requests go to the configured endpoint in the same shapes
//! real calls use: a one-sentence extraction (text), a 1×1 PNG (image) and
//! a one-second silent WAV (audio). Each is accepted when the endpoint
//! answers with message content, and reports how long that took. The check ignores the capabilities the
//! endpoint was last recorded with, since it is what measures them, but
//! still crosses the private allowlist before any socket opens.

use std::time::Instant;

use office_core::Result;
use office_core::domain::endpoint::Capability;
use office_core::ports::{RouteCheck, RouteProbe};
use serde_json::{Value, json};

use crate::{RoutedClient, http, meta, open_weights, routing};

/// A valid 1×1 transparent PNG.
const PNG_1X1: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Longest reported model name kept in the result.
const MAX_REPORTED_MODEL: usize = 200;

pub(crate) fn check(client: &RoutedClient) -> Result<RouteCheck> {
    let resolved = routing::private_endpoint(&client.config)?;
    // A host outside the allowlist is a refusal, not a missing capability.
    http::check_url(&meta::chat_url(&resolved.base_url), resolved.destination())?;
    let mut probes = Vec::with_capacity(Capability::ALL.len());
    let mut reported_model = None;
    for capability in Capability::ALL {
        let body = probe_body(capability, &resolved.model);
        let started = Instant::now();
        let outcome = client.send(&resolved, &body).and_then(|(host, response)| {
            meta::reply_text(&host, &response)?;
            Ok(response)
        });
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let error = match outcome {
            Ok(response) => {
                if reported_model.is_none() {
                    reported_model = reported(&response, resolved.api_key.as_deref());
                }
                None
            }
            Err(error) => {
                tracing::debug!(
                    capability = capability.as_str(),
                    code = error.code(),
                    "private route check request failed"
                );
                Some(error.code().to_string())
            }
        };
        probes.push(RouteProbe {
            capability,
            accepted: error.is_none(),
            elapsed_ms: error.is_none().then_some(elapsed),
            error,
        });
    }
    Ok(RouteCheck {
        endpoint: resolved.base_url,
        model: resolved.model,
        reported_model,
        probes,
    })
}

fn probe_body(capability: Capability, model: &str) -> Value {
    match capability {
        Capability::Text => open_weights::extract_body(
            model,
            "The weekly review is on Tuesday.",
            &json!({"type": "object"}),
        ),
        Capability::Image => meta::image_body(
            model,
            "image/png",
            &PNG_1X1,
            "Describe this image in one word.",
        ),
        Capability::Audio => meta::transcribe_body(model, "wav", &silent_wav(), None),
    }
}

/// One second of 8 kHz, 16-bit mono PCM silence.
fn silent_wav() -> Vec<u8> {
    const RATE: u32 = 8_000;
    const DATA_BYTES: u32 = RATE * 2;
    let mut wav = Vec::with_capacity(44 + 16_000);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + DATA_BYTES).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2_u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&DATA_BYTES.to_le_bytes());
    wav.resize(44 + 16_000, 0);
    wav
}

/// The reply's `model` field, when it is a short single token that does not
/// echo the endpoint's credential back (a hostile endpoint could).
fn reported(response: &Value, token: Option<&str>) -> Option<String> {
    response
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| {
            !model.is_empty()
                && model.chars().count() <= MAX_REPORTED_MODEL
                && !model.chars().any(|c| c.is_whitespace() || c.is_control())
                && token.is_none_or(|token| token.is_empty() || !model.contains(token))
        })
        .map(str::to_string)
}
