//! Model-endpoint host policy and the private route's vocabulary, shared by
//! Library settings and `office-models`' `check_url`
//! (specs/models-and-privacy.md, "The private route").
//!
//! Hosted calls go only to `api.meta.ai` (or loopback, which only a test
//! configures). Private calls go only to the one configured open-weights
//! endpoint: the same scheme, exact host and port, over HTTPS unless the
//! host is loopback, and never the hosted Meta API. Any other host is
//! refused, at settings load and again before a socket opens.

use std::fmt;

use serde::Serialize;

/// The hosted Meta Model API host.
pub const META_HOST: &str = "api.meta.ai";

/// Where a private endpoint runs; shown with its label wherever it is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// The user's company's own deployment.
    Company,
    /// A hosted provider (a third party, not Meta's hosted API).
    Provider,
    /// Machines the user owns.
    OwnHardware,
}

impl EndpointKind {
    /// Every kind, in documentation order.
    pub const ALL: [Self; 3] = [Self::Company, Self::Provider, Self::OwnHardware];

    /// The settings spelling: `company`, `provider` or `own_hardware`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Provider => "provider",
            Self::OwnHardware => "own_hardware",
        }
    }

    /// Parse the settings spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// An input modality the private endpoint was checked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Text in (every private call needs it).
    Text,
    /// Image in (frame and receipt reading).
    Image,
    /// Audio in (transcription).
    Audio,
}

impl Capability {
    /// Every capability, in check order.
    pub const ALL: [Self; 3] = [Self::Text, Self::Image, Self::Audio];

    /// The settings spelling: `text`, `image` or `audio`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }

    /// Parse the settings spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|cap| cap.as_str() == text)
    }
}

/// Where one model call went: `hosted` or `private:<kind>`, as recorded in
/// `derived_by.route`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRoute {
    /// The hosted Meta Model API.
    Hosted,
    /// The configured private endpoint of this kind.
    Private(EndpointKind),
}

impl fmt::Display for ModelRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hosted => f.write_str("hosted"),
            Self::Private(kind) => write!(f, "private:{}", kind.as_str()),
        }
    }
}

impl Serialize for ModelRoute {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Why a model URL is outside the transport allowlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelUrlError {
    /// Not an `http://` or `https://` URL, or the host is missing or malformed.
    NotHttpUrl,
    /// The host is not one this call may reach.
    HostNotAllowed {
        /// Lowercase host, without port or brackets.
        host: String,
    },
    /// A non-loopback host did not use HTTPS.
    HttpsRequired {
        /// Lowercase host, without port or brackets.
        host: String,
    },
}

/// Refusal reported for a private endpoint over plain HTTP off loopback.
/// Static, so a settings error never echoes the configured URL.
const HTTPS_REQUIRED: &str = "expected an HTTPS URL (plain HTTP only on loopback)";
const META_REFUSED: &str =
    "expected the endpoint you chose, not Meta's hosted API (api.meta.ai is never private)";
const MALFORMED_ENDPOINT: &str =
    "expected an HTTP(S) base URL with a host, no credentials, query or fragment";

/// The host part of an `http(s)://` URL, without port, userinfo or the
/// brackets of an IPv6 literal (`[::1]:8080` is `::1`).
#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let host = authority_parts(url)?.0;
    if host.is_empty() { None } else { Some(host) }
}

/// Lowercase host and explicit port (if any) of an `http(s)://` URL.
fn authority_parts(url: &str) -> Option<(String, Option<String>)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_and_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = match host_and_port.strip_prefix('[') {
        // An unclosed bracket is malformed: no host, so the URL is refused.
        Some(literal) => {
            let (host, after) = literal.split_once(']')?;
            (host, after.strip_prefix(':'))
        }
        None => match host_and_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_and_port, None),
        },
    };
    Some((host.to_ascii_lowercase(), port.map(str::to_string)))
}

/// `https` or `http`, the explicit-or-default port, and the lowercase host:
/// the origin a request to `url` opens a socket to.
fn origin_of(url: &str) -> Option<(&'static str, String, String)> {
    let scheme = if url.starts_with("https://") {
        "https"
    } else if url.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let (host, port) = authority_parts(url)?;
    if host.is_empty() {
        return None;
    }
    let port = port.unwrap_or_else(|| if scheme == "https" { "443" } else { "80" }.into());
    Some((scheme, host, port))
}

/// `localhost`, `127.0.0.1` or `::1` (already lowercase, no brackets).
#[must_use]
pub fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// The hosted allowlist: `api.meta.ai` over HTTPS, or a loopback host
/// (only a test points the hosted base URL there). Returns the host.
pub fn allow_hosted_url(url: &str) -> Result<String, ModelUrlError> {
    let host = host_of(url).ok_or(ModelUrlError::NotHttpUrl)?;
    if is_loopback(&host) {
        return Ok(host);
    }
    if host != META_HOST {
        return Err(ModelUrlError::HostNotAllowed { host });
    }
    if !url.starts_with("https://") {
        return Err(ModelUrlError::HttpsRequired { host });
    }
    Ok(host)
}

/// The private allowlist: `url` must open a socket to exactly the origin
/// (scheme, host and port) of the configured `endpoint`, over HTTPS unless
/// loopback, and never to `api.meta.ai`. Returns the host.
pub fn allow_private_url(url: &str, endpoint: &str) -> Result<String, ModelUrlError> {
    let (scheme, host, port) = origin_of(url).ok_or(ModelUrlError::NotHttpUrl)?;
    let Some(configured) = origin_of(endpoint) else {
        return Err(ModelUrlError::HostNotAllowed { host });
    };
    if host == META_HOST || (scheme, &host, &port) != (configured.0, &configured.1, &configured.2) {
        return Err(ModelUrlError::HostNotAllowed { host });
    }
    if scheme != "https" && !is_loopback(&host) {
        return Err(ModelUrlError::HttpsRequired { host });
    }
    Ok(host)
}

/// Accept a configured private endpoint, or the static reason settings
/// should report. Any host is allowed (the user chose it), over HTTPS
/// unless loopback, except Meta's hosted API. Embedded credentials, a
/// query or a fragment are refused. The returned reason never contains
/// the URL.
pub(crate) fn allow_private_endpoint(url: &str) -> Result<(), &'static str> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control())
        || url.contains(['?', '#'])
        || authority_has_userinfo(url)
        || !has_valid_authority(url)
    {
        return Err(MALFORMED_ENDPOINT);
    }
    let host = host_of(url).ok_or(MALFORMED_ENDPOINT)?;
    if host == META_HOST {
        return Err(META_REFUSED);
    }
    if !url.starts_with("https://") && !is_loopback(&host) {
        return Err(HTTPS_REQUIRED);
    }
    Ok(())
}

/// A port is usable only as non-empty ASCII digits fitting a `u16`.
fn is_valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

/// The authority is syntactically usable: a bracketed IPv6 literal may be
/// followed only by nothing or a valid port, and any other host may carry
/// at most one valid port. Extracting an allowlisted host does not prove
/// the remaining authority is valid, so it is checked in full here.
fn has_valid_authority(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_and_port = authority.rsplit('@').next().unwrap_or_default();
    if let Some(literal) = host_and_port.strip_prefix('[') {
        let Some((host, after)) = literal.split_once(']') else {
            return false;
        };
        if host.parse::<core::net::Ipv6Addr>().is_err() {
            return false;
        }
        return match after {
            "" => true,
            _ => after.strip_prefix(':').is_some_and(is_valid_port),
        };
    }
    if host_and_port.contains(['[', ']']) {
        return false;
    }
    match host_and_port.split_once(':') {
        None => !host_and_port.is_empty(),
        Some((host, port)) => !host.is_empty() && is_valid_port(port),
    }
}

/// `userinfo@host` in the authority. Credentials do not belong in settings.
fn authority_has_userinfo(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    rest.split('/').next().unwrap_or_default().contains('@')
}
