//! The one HTTP helper every model call crosses: host allowlist first,
//! then one blocking JSON POST.
//!
//! [`check_url`] allows only:
//! - for hosted calls, `api.meta.ai` over HTTPS (or loopback, which only a
//!   test configures as the hosted base URL);
//! - for private calls, exactly the configured endpoint's origin (scheme,
//!   host and port), over HTTPS unless it is loopback, and never
//!   `api.meta.ai`.
//!
//! Any other host, a Baseten host that is not the configured one
//! included, is refused before a socket opens. Library settings apply the
//! same endpoint rule at load.
//!
//! Errors name only the host and status: never a credential, request
//! body or response body (STD-05), because a hostile endpoint can echo
//! the credential back in its response body.

use office_core::Error;
use office_core::domain::endpoint::{self, ModelUrlError};
use serde_json::Value;

/// The allowlist one request is checked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination<'a> {
    /// The hosted Meta Model API.
    Hosted,
    /// The configured private endpoint (its base URL).
    Private {
        /// The configured base URL whose origin the request must match.
        endpoint: &'a str,
    },
}

/// The host part of an `http(s)://` URL, without port, userinfo or the
/// brackets of an IPv6 literal (`[::1]:8080` is `::1`).
pub(crate) fn host_of(url: &str) -> Option<String> {
    endpoint::host_of(url)
}

/// Whether `url` may be contacted for `destination`. Returns the host.
pub(crate) fn check_url(url: &str, destination: Destination<'_>) -> Result<String, Error> {
    let checked = match destination {
        Destination::Hosted => endpoint::allow_hosted_url(url),
        Destination::Private { endpoint } => endpoint::allow_private_url(url, endpoint),
    };
    checked.map_err(|error| match error {
        ModelUrlError::NotHttpUrl => {
            refused("model endpoint is not an HTTP(S) URL with a host".to_string())
        }
        ModelUrlError::HostNotAllowed { host } => refused(match destination {
            Destination::Hosted => format!(
                "model endpoint host {host} is not the Meta Model API host; refusing the call"
            ),
            Destination::Private { .. } => format!(
                "model endpoint host {host} is not the configured private endpoint; refusing the call"
            ),
        }),
        ModelUrlError::HttpsRequired { host } => {
            refused(format!("model endpoint host {host} must use HTTPS"))
        }
    })
}

fn refused(message: String) -> Error {
    Error::Model {
        code: "model_host_not_allowed",
        message,
        retryable: false,
    }
}

/// POST `body` as JSON to `url` with an optional Bearer credential and
/// return the parsed JSON response. The URL is allowlist-checked against
/// `destination` first. The routed client's agent disables redirects so a
/// response cannot cause a request to an unchecked host.
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    api_key: Option<&str>,
    body: &Value,
    destination: Destination<'_>,
) -> Result<Value, Error> {
    let host = check_url(url, destination)?;
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
