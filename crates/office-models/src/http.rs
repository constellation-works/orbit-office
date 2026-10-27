//! The one HTTP helper every model call crosses: host allowlist first,
//! then one blocking JSON POST. Only the Meta and Baseten hosts are
//! reachable; any other host is refused in code before a socket opens.
//! A configured open-weights endpoint is reachable by that configuration,
//! and loopback is reachable only as an explicitly configured endpoint
//! (the loopback fake server in tests, a local open-weights server).
//!
//! Errors name only the host and status: never a credential, request
//! body or response body (STD-05), because a hostile endpoint can echo
//! the credential back in its response body.

use office_core::Error;
use serde_json::Value;

/// Hosts reachable without further configuration.
const ALLOWED_HOSTS: &[&str] = &["api.meta.ai", "inference.baseten.co"];

/// The host part of an `http(s)://` URL, without port or userinfo.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Whether `url` may be contacted at all. Non-loopback endpoints must use
/// HTTPS; loopback endpoints (a configured local model, the test server)
/// may use HTTP.
pub(crate) fn check_url(url: &str) -> Result<String, Error> {
    let host = host_of(url).ok_or_else(|| Error::Model {
        code: "model_host_not_allowed",
        message: "model endpoint is not an HTTP(S) URL with a host".to_string(),
        retryable: false,
    })?;
    let allowed = ALLOWED_HOSTS.contains(&host.as_str())
        || host.ends_with(".api.baseten.co")
        || is_loopback(&host);
    if !allowed {
        return Err(Error::Model {
            code: "model_host_not_allowed",
            message: format!(
                "model endpoint host {host} is not a Meta or Baseten host; refusing the call"
            ),
            retryable: false,
        });
    }
    if !is_loopback(&host) && !url.starts_with("https://") {
        return Err(Error::Model {
            code: "model_host_not_allowed",
            message: format!("model endpoint host {host} must use HTTPS"),
            retryable: false,
        });
    }
    Ok(host)
}

/// POST `body` as JSON to `url` with an optional Bearer credential and
/// return the parsed JSON response. The URL is allowlist-checked first.
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    api_key: Option<&str>,
    body: &Value,
) -> Result<Value, Error> {
    let host = check_url(url)?;
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
