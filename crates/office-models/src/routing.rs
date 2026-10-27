//! Privacy routing: `private` material goes only to the configured private
//! endpoint, never a hosted model (specs/models-and-privacy.md).
//!
//! [`route`] is the one routing function every model call crosses
//! (STD-02 R24): adapters never pick an endpoint themselves. Every tier and
//! modality of a private call goes to the one configured model, and only
//! once `office.private-route-check` has recorded that the endpoint takes
//! text (and the call's modality).

use office_common::Sensitivity;
use office_core::Error;
use office_core::domain::endpoint::{Capability, ModelRoute};
use office_core::ports::DerivedBy;

use crate::http::Destination;

/// Where one call will be sent, resolved before any network I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedCall {
    /// Provider base URL, without a trailing slash.
    pub base_url: String,
    /// Bearer credential for that endpoint, when it needs one.
    pub api_key: Option<String>,
    /// Model slug to name in the request.
    pub model: String,
    /// `hosted` or `private:<kind>`.
    pub route: ModelRoute,
}

impl ResolvedCall {
    /// The allowlist this call is checked against before a socket opens.
    pub(crate) fn destination(&self) -> Destination<'_> {
        match self.route {
            ModelRoute::Hosted => Destination::Hosted,
            ModelRoute::Private(_) => Destination::Private {
                endpoint: &self.base_url,
            },
        }
    }

    /// The `derived_by` fields a result of this call carries.
    pub(crate) fn derived_by(&self) -> DerivedBy {
        DerivedBy {
            route: self.route,
            model: self.model.clone(),
        }
    }

    /// Whether the call targets the private endpoint.
    pub(crate) fn is_private(&self) -> bool {
        matches!(self.route, ModelRoute::Private(_))
    }
}

/// The one routing decision (STD-02 R24) for a call needing `needs`.
///
/// `Normal` material goes to the hosted Meta endpoint. `Private` material
/// goes only to the configured private endpoint, and only when its last
/// check accepted text and `needs`. With no endpoint configured the call is
/// refused here and never falls back to a hosted model.
pub(crate) fn route(
    sensitivity: Sensitivity,
    needs: Capability,
    config: &crate::ModelConfig,
) -> Result<ResolvedCall, Error> {
    match sensitivity {
        Sensitivity::Normal => {
            let api_key = config.meta_api_key.clone().ok_or_else(|| Error::Model {
                code: "model_unavailable",
                message: "Meta Model API key is not configured".to_string(),
                retryable: false,
            })?;
            Ok(ResolvedCall {
                base_url: config.meta_base_url.trim_end_matches('/').to_string(),
                api_key: Some(api_key),
                model: config.hosted_model.clone(),
                route: ModelRoute::Hosted,
            })
        }
        Sensitivity::Private => {
            let block = config.open_weights.as_ref().ok_or_else(unconfigured)?;
            if !block.accepts(Capability::Text) {
                return Err(Error::Model {
                    code: "private_endpoint_unverified",
                    message: "the private endpoint has not passed office.private-route-check's text request; nothing is sent to it".to_string(),
                    retryable: false,
                });
            }
            if !block.accepts(needs) {
                return Err(Error::Model {
                    code: "private_media_unsupported",
                    message: match needs {
                        Capability::Image => "the private endpoint doesn't take images (needs: image), so private images and frames can't be read",
                        Capability::Audio => "the private endpoint doesn't take audio (needs: audio), so private recordings can't be transcribed",
                        Capability::Text => "the private endpoint doesn't take text (needs: text)",
                    }
                    .to_string(),
                    retryable: false,
                });
            }
            private_endpoint(config)
        }
    }
}

/// The configured private endpoint, whatever its last check found. Only
/// `office.private-route-check` sends there without [`route`]'s
/// capability gate, because it is what measures the capabilities.
pub(crate) fn private_endpoint(config: &crate::ModelConfig) -> Result<ResolvedCall, Error> {
    let block = config.open_weights.as_ref().ok_or_else(unconfigured)?;
    Ok(ResolvedCall {
        base_url: block.endpoint.trim_end_matches('/').to_string(),
        api_key: config.open_weights_token.clone(),
        model: block.model.clone(),
        route: ModelRoute::Private(block.kind),
    })
}

fn unconfigured() -> Error {
    Error::Model {
        code: "private_endpoint_unconfigured",
        message: "private material needs an open_weights endpoint in Library settings; refusing to use the hosted model".to_string(),
        retryable: false,
    }
}
