//! Model-endpoint host policy and the private route's vocabulary, shared by
//! Library settings and `office-models`' `check_url`
//! (specs/models-and-privacy.md, "The private route").
//!
//! Hosted calls go only to the configured hosted endpoint
//! (`models.endpoint`, by default Meta's [`DEFAULT_HOSTED_ENDPOINT`]).
//! Private calls go only to the one configured open-weights endpoint and
//! never to Meta's hosted API. Either way a call must open a socket to
//! exactly the configured origin (scheme, host and port), over HTTPS unless
//! the host is loopback, and a configured endpoint never carries
//! credentials, a query or a fragment. Any other host is refused, at
//! settings load and again before a socket opens.

use std::fmt;

use serde::Serialize;

/// The hosted Meta Model API host. Private endpoints may never be it.
pub const META_HOST: &str = "api.meta.ai";

/// The hosted endpoint used when `models.endpoint` is unset: Meta's
/// OpenAI-compatible Model API.
pub const DEFAULT_HOSTED_ENDPOINT: &str = "https://api.meta.ai/v1";

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
    /// The configured hosted endpoint (`models.endpoint`).
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

/// Refusal reported for an endpoint over plain HTTP off loopback.
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

/// The hosted allowlist: `url` must open a socket to exactly the origin
/// (scheme, host and port) of the configured hosted `endpoint`, over HTTPS
/// unless loopback, with no credentials in its authority. The endpoint
/// itself must pass the settings rule ([`allow_hosted_endpoint`]), so a
/// malformed or credential-bearing endpoint reaches nothing. Returns the
/// host.
pub fn allow_hosted_url(url: &str, endpoint: &str) -> Result<String, ModelUrlError> {
    let (scheme, host, port) = origin_of(url).ok_or(ModelUrlError::NotHttpUrl)?;
    if authority_has_userinfo(url) {
        return Err(ModelUrlError::NotHttpUrl);
    }
    let configured = match allow_hosted_endpoint(endpoint) {
        Ok(()) => origin_of(endpoint),
        Err(_) => None,
    };
    let Some(configured) = configured else {
        return Err(ModelUrlError::HostNotAllowed { host });
    };
    if (scheme, &host, &port) != (configured.0, &configured.1, &configured.2) {
        return Err(ModelUrlError::HostNotAllowed { host });
    }
    if scheme != "https" && !is_loopback(&host) {
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

/// Accept a configured hosted endpoint (`models.endpoint`), or the static
/// reason settings should report. Any OpenAI-compatible host is allowed
/// (the user chose it), over HTTPS unless loopback. Embedded credentials,
/// a query or a fragment are refused. The returned reason never contains
/// the URL.
pub fn allow_hosted_endpoint(url: &str) -> Result<(), &'static str> {
    let host = well_formed_host(url)?;
    require_https_off_loopback(url, &host)
}

/// Accept a configured private endpoint, or the static reason settings
/// should report: the hosted rule ([`allow_hosted_endpoint`]), except that
/// Meta's hosted API is never private. The returned reason never contains
/// the URL.
pub(crate) fn allow_private_endpoint(url: &str) -> Result<(), &'static str> {
    let host = well_formed_host(url)?;
    if host == META_HOST {
        return Err(META_REFUSED);
    }
    require_https_off_loopback(url, &host)
}

/// The lowercase host of a well-formed HTTP(S) base URL with no
/// credentials, query or fragment.
fn well_formed_host(url: &str) -> Result<String, &'static str> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control())
        || url.contains(['?', '#'])
        || authority_has_userinfo(url)
        || !has_valid_authority(url)
    {
        return Err(MALFORMED_ENDPOINT);
    }
    host_of(url).ok_or(MALFORMED_ENDPOINT)
}

/// Plain HTTP only on loopback.
fn require_https_off_loopback(url: &str, host: &str) -> Result<(), &'static str> {
    if !url.starts_with("https://") && !is_loopback(host) {
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

/// The private route's stored credential (`open_weights_token`): the
/// token and the endpoint origin it was saved for, as one JSON record
/// `{"origin": "https://host[:port]", "token": "<token>"}` written in one
/// secret call (specs/models-and-privacy.md, "The private route's token").
///
/// The token is only ever paired with an endpoint whose origin (scheme,
/// host and port) equals the bound one. Library config cannot forge the
/// binding: `office.config-set` never writes a secret. A stored value that
/// is not such a record (for example a bare token set outside Settings) is
/// not bound to any origin and is never sent.
#[derive(Clone, PartialEq, Eq)]
pub struct BoundToken {
    origin: String,
    token: String,
}

impl BoundToken {
    /// Parse the stored record. Anything else, including a bare token, an
    /// empty token, an extra key or an origin that is not an HTTP(S) URL,
    /// is `None`: no binding, so nothing to send.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        let serde_json::Value::Object(record) = serde_json::from_str(stored).ok()? else {
            return None;
        };
        if record.len() != 2 {
            return None;
        }
        let origin = record.get("origin")?.as_str()?;
        let token = record.get("token")?.as_str()?;
        if token.is_empty() || origin_of(origin).is_none() {
            return None;
        }
        Some(Self {
            origin: origin.to_string(),
            token: token.to_string(),
        })
    }

    /// The token, only when `endpoint` opens a socket to the bound origin.
    #[must_use]
    pub fn token_for(&self, endpoint: &str) -> Option<&str> {
        let bound = origin_of(&self.origin)?;
        (origin_of(endpoint)? == bound).then_some(self.token.as_str())
    }
}

impl fmt::Debug for BoundToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The token never reaches a log or an error, even through Debug.
        f.debug_struct("BoundToken")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// How the stored private-route token relates to the configured endpoint,
/// as `office.status` reports it (`open_weights_token_binding`). Never
/// the token or its origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenBinding {
    /// No token is stored.
    Unset,
    /// Saved for the configured endpoint's origin: calls there send it.
    ThisEndpoint,
    /// Saved for another origin, or no endpoint is configured: nothing
    /// sends it until the endpoint is back on that origin.
    OtherEndpoint,
    /// Stored without an endpoint binding (for example a bare token):
    /// never sent.
    Unbound,
}

/// The hosted route's stored credential (`meta_api_key`): the key and
/// the endpoint origin it was saved for, as one JSON record
/// `{"origin": "https://host[:port]", "key": "<key>"}` written in one
/// secret call (specs/models-and-privacy.md, "The hosted route").
///
/// The key is only ever paired with an endpoint whose origin (scheme,
/// host and port) equals the bound one. Library config cannot forge the
/// binding: `office.config-set` never writes a secret. A stored value
/// that is not such a record (for example a bare key set outside
/// Settings) is not bound to any origin and is never sent; it is bound
/// to the current origin only when it is saved again, never on read.
#[derive(Clone, PartialEq, Eq)]
pub struct BoundHostedKey {
    origin: String,
    key: String,
}

impl BoundHostedKey {
    /// Parse the stored record. Anything else, including a bare key, an
    /// empty key, an extra key or an origin that is not an HTTP(S) URL,
    /// is `None`: no binding, so nothing to send.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        let serde_json::Value::Object(record) = serde_json::from_str(stored).ok()? else {
            return None;
        };
        if record.len() != 2 {
            return None;
        }
        let origin = record.get("origin")?.as_str()?;
        let key = record.get("key")?.as_str()?;
        if key.is_empty() || origin_of(origin).is_none() {
            return None;
        }
        Some(Self {
            origin: origin.to_string(),
            key: key.to_string(),
        })
    }

    /// The origin the key was saved for, exactly as stored. This is not
    /// secret: `office.status` reports it so the binding can be checked.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The key, only when `endpoint` opens a socket to the bound origin.
    #[must_use]
    pub fn key_for(&self, endpoint: &str) -> Option<&str> {
        let bound = origin_of(&self.origin)?;
        (origin_of(endpoint)? == bound).then_some(self.key.as_str())
    }
}

impl fmt::Debug for BoundHostedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The key never reaches a log or an error, even through Debug.
        f.debug_struct("BoundHostedKey")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// How the stored hosted key relates to the configured hosted endpoint,
/// as `office.status` reports it (`hosted_key_binding`). The bound
/// origin itself is reported separately (`hosted_key_bound_origin`);
/// never the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedKeyBinding {
    /// No key is stored.
    Unset,
    /// Saved for the configured endpoint's origin: calls there send it.
    ThisEndpoint,
    /// Saved for another origin: calls are refused until the endpoint
    /// is back on that origin or the key is saved again for this one.
    OtherEndpoint,
    /// Stored without an endpoint binding (for example a bare key):
    /// never sent.
    Unbound,
}

impl HostedKeyBinding {
    /// Classify the stored value against the configured endpoint, if any.
    #[must_use]
    pub fn of(stored: Option<&str>, endpoint: Option<&str>) -> Self {
        let Some(stored) = stored.filter(|value| !value.is_empty()) else {
            return Self::Unset;
        };
        let Some(bound) = BoundHostedKey::parse(stored) else {
            return Self::Unbound;
        };
        match endpoint.and_then(|endpoint| bound.key_for(endpoint)) {
            Some(_) => Self::ThisEndpoint,
            None => Self::OtherEndpoint,
        }
    }

    /// The status spelling: `unset`, `this_endpoint`, `other_endpoint` or
    /// `unbound`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::ThisEndpoint => "this_endpoint",
            Self::OtherEndpoint => "other_endpoint",
            Self::Unbound => "unbound",
        }
    }
}

impl TokenBinding {
    /// Classify the stored value against the configured endpoint, if any.
    #[must_use]
    pub fn of(stored: Option<&str>, endpoint: Option<&str>) -> Self {
        let Some(stored) = stored.filter(|value| !value.is_empty()) else {
            return Self::Unset;
        };
        let Some(bound) = BoundToken::parse(stored) else {
            return Self::Unbound;
        };
        match endpoint.and_then(|endpoint| bound.token_for(endpoint)) {
            Some(_) => Self::ThisEndpoint,
            None => Self::OtherEndpoint,
        }
    }

    /// The status spelling: `unset`, `this_endpoint`, `other_endpoint` or
    /// `unbound`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::ThisEndpoint => "this_endpoint",
            Self::OtherEndpoint => "other_endpoint",
            Self::Unbound => "unbound",
        }
    }
}
