//! Model-endpoint host policy shared by Library settings and `check_url`.
//!
//! The transport allowlist is Meta, Baseten, or loopback. An open-weights
//! endpoint is the Baseten or loopback subset: Baseten over HTTPS, loopback
//! over HTTP or HTTPS. Any other host is refused. Hosted calls may use
//! `api.meta.ai`; that host is not an open-weights endpoint.

/// Why a model URL is outside the transport allowlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelUrlError {
    /// Not an `http://` or `https://` URL, or the host is missing or malformed.
    NotHttpUrl,
    /// The host is not Meta, Baseten, or loopback.
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

/// Which transport-allowlist role a host fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelHostRole {
    /// `api.meta.ai`. Hosted calls only.
    Meta,
    /// `inference.baseten.co` or a host ending in `.api.baseten.co`.
    Baseten,
    /// `localhost`, `127.0.0.1`, or `::1`.
    Loopback,
}

impl ModelHostRole {
    /// Open-weights endpoints are Baseten or loopback, never the hosted Meta API.
    #[must_use]
    pub const fn is_open_weights(self) -> bool {
        matches!(self, Self::Baseten | Self::Loopback)
    }
}

/// Refusal reported for an open-weights URL outside the Baseten-or-loopback rule.
///
/// Static, so a settings error never echoes the configured URL.
pub(crate) const OPEN_WEIGHTS_ENDPOINT_REASON: &str =
    "expected a Baseten HTTPS endpoint or a loopback HTTP(S) endpoint";

const MALFORMED_ENDPOINT: &str = "expected an HTTP(S) URL with a host and no credentials";

/// The host part of an `http(s)://` URL, without port, userinfo or the
/// brackets of an IPv6 literal (`[::1]:8080` is `::1`).
#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    let host_and_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_and_port.strip_prefix('[') {
        // An unclosed bracket is malformed: no host, so the URL is refused.
        Some(literal) => literal.split_once(']')?.0,
        None => host_and_port.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Classify `host` (already lowercase, without port or brackets).
#[must_use]
pub(crate) fn model_host_role(host: &str) -> Option<ModelHostRole> {
    if host == "api.meta.ai" {
        Some(ModelHostRole::Meta)
    } else if host == "inference.baseten.co" || host.ends_with(".api.baseten.co") {
        Some(ModelHostRole::Baseten)
    } else if matches!(host, "127.0.0.1" | "localhost" | "::1") {
        Some(ModelHostRole::Loopback)
    } else {
        None
    }
}

/// Transport allowlist used by `check_url`: Meta, Baseten, or loopback.
///
/// Non-loopback endpoints must use HTTPS. Returns the lowercase host and
/// its role.
pub fn allow_model_url(url: &str) -> Result<(String, ModelHostRole), ModelUrlError> {
    let host = host_of(url).ok_or(ModelUrlError::NotHttpUrl)?;
    let role = model_host_role(&host)
        .ok_or_else(|| ModelUrlError::HostNotAllowed { host: host.clone() })?;
    if !matches!(role, ModelHostRole::Loopback) && !url.starts_with("https://") {
        return Err(ModelUrlError::HttpsRequired { host });
    }
    Ok((host, role))
}

/// Accept an open-weights endpoint, or the static reason settings should report.
///
/// Embedded credentials are refused even when the host would otherwise be
/// allowed. The returned reason never contains the URL.
pub(crate) fn allow_open_weights_endpoint(url: &str) -> Result<(), &'static str> {
    if url.chars().any(char::is_whitespace) || authority_has_userinfo(url) {
        return Err(MALFORMED_ENDPOINT);
    }
    match allow_model_url(url) {
        Ok((_, role)) if role.is_open_weights() => Ok(()),
        Ok(_) | Err(ModelUrlError::HostNotAllowed { .. } | ModelUrlError::HttpsRequired { .. }) => {
            Err(OPEN_WEIGHTS_ENDPOINT_REASON)
        }
        Err(ModelUrlError::NotHttpUrl) => Err(MALFORMED_ENDPOINT),
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
