//! The one HTTP helper every model call crosses: host allowlist first,
//! then one blocking JSON POST.
//!
//! [`check_url`] allows only:
//! - `api.meta.ai`, for hosted calls;
//! - a Baseten host (`inference.baseten.co`, or a host ending in
//!   `.api.baseten.co`);
//! - loopback (`localhost`, `127.0.0.1`, `::1`), only as an explicitly
//!   configured endpoint (a local open-weights server, or the test server).
//!
//! An open-weights endpoint is that Baseten or loopback subset, not an
//! arbitrary configured host and not the hosted Meta API. Any other host
//! is refused before a socket opens. Non-loopback endpoints must use
//! HTTPS; loopback may use HTTP. Library settings apply the same
//! open-weights rule at load.
//!
//! Errors name only the host and status: never a credential, request
//! body or response body (STD-05), because a hostile endpoint can echo
//! the credential back in its response body.

use office_core::Error;
use office_core::domain::endpoint::{self, ModelUrlError};
use serde_json::Value;

/// The host part of an `http(s)://` URL, without port, userinfo or the
/// brackets of an IPv6 literal (`[::1]:8080` is `::1`).
pub(crate) fn host_of(url: &str) -> Option<String> {
    endpoint::host_of(url)
}

/// Whether `url` may be contacted. Non-loopback endpoints must use HTTPS;
/// loopback endpoints (a configured local model, the test server) may use
/// HTTP. `open_weights` keeps only the Baseten or loopback subset; hosted
/// calls pass `false` and may use `api.meta.ai`.
pub(crate) fn check_url(url: &str, open_weights: bool) -> Result<String, Error> {
    match endpoint::allow_model_url(url) {
        Ok((host, role)) if open_weights && !role.is_open_weights() => Err(refused(format!(
            "open-weights endpoint host {host} is not a Baseten or loopback host"
        ))),
        Ok((host, _)) => Ok(host),
        Err(ModelUrlError::NotHttpUrl) => Err(refused(
            "model endpoint is not an HTTP(S) URL with a host".to_string(),
        )),
        Err(ModelUrlError::HostNotAllowed { host }) => Err(refused(format!(
            "model endpoint host {host} is not a Meta or Baseten host; refusing the call"
        ))),
        Err(ModelUrlError::HttpsRequired { host }) => Err(refused(format!(
            "model endpoint host {host} must use HTTPS"
        ))),
    }
}

fn refused(message: String) -> Error {
    Error::Model {
        code: "model_host_not_allowed",
        message,
        retryable: false,
    }
}

/// POST `body` as JSON to `url` with an optional Bearer credential and
/// return the parsed JSON response. The URL is allowlist-checked first.
/// `open_weights` applies the Baseten-or-loopback role on top of the
/// transport allowlist.
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    api_key: Option<&str>,
    body: &Value,
    open_weights: bool,
) -> Result<Value, Error> {
    let host = check_url(url, open_weights)?;
    tracing::debug!(host = %host, "model call");
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut response = request
        .send_json(body)
        .map_err(|error| map_error(&host, error))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|_| Error::Model {
            code: "model_invalid_response",
            message: format!("model endpoint {host} returned an unreadable response"),
            retryable: false,
        })?;
    serde_json::from_str(&text).map_err(|_| Error::Model {
        code: "model_invalid_response",
        message: format!("model endpoint {host} returned a non-JSON response"),
        retryable: false,
    })
}

fn map_error(host: &str, error: ureq::Error) -> Error {
    // Foreign `#[non_exhaustive]`-style enum: every non-status failure is
    // the same retryable transport class here (STD-02 R27 scoped allow).
    #[allow(
        clippy::wildcard_enum_match_arm,
        reason = "foreign ureq::Error: all non-status variants are one retryable transport class"
    )]
    match error {
        ureq::Error::StatusCode(status) => {
            let retryable = status == 429 || (500..=599).contains(&status);
            tracing::warn!(host = %host, status, retryable, "model call failed");
            Error::Model {
                code: if retryable {
                    "model_unavailable"
                } else {
                    "model_request_rejected"
                },
                message: format!("model endpoint {host} returned HTTP {status}"),
                retryable,
            }
        }
        _ => {
            tracing::warn!(host = %host, "model call unreachable");
            Error::Model {
                code: "model_unavailable",
                message: format!("could not reach model endpoint {host}"),
                retryable: true,
            }
        }
    }
}
