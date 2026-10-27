//! Privacy routing: `private` material goes to the open-weights endpoint,
//! never a hosted model (specs/models-and-privacy.md).
//!
//! [`route`] is the one routing function every model call crosses
//! (STD-02 R24): adapters never pick an endpoint themselves.

use office_common::Sensitivity;
use office_core::Error;

/// Where one call will be sent, resolved before any network I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedCall {
    /// Provider base URL, without a trailing slash.
    pub base_url: String,
    /// Bearer credential for that endpoint, when it needs one.
    pub api_key: Option<String>,
    /// Model slug to name in the request.
    pub model: String,
    /// True when the call targets the open-weights endpoint.
    pub open_weights: bool,
}

/// The one routing decision (STD-02 R24).
///
/// `Normal` material goes to the hosted Meta endpoint. `Private` material
/// goes only to the configured open-weights endpoint; with none configured
/// the call is refused here and never falls back to a hosted model.
pub(crate) fn route(
    sensitivity: Sensitivity,
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
                open_weights: false,
            })
        }
        Sensitivity::Private => {
            let endpoint =
                config
                    .open_weights_endpoint
                    .clone()
                    .ok_or_else(|| Error::Model {
                        code: "private_endpoint_unconfigured",
                        message: "private material needs an open-weights endpoint in Library settings; refusing to use a hosted model".to_string(),
                        retryable: false,
                    })?;
            Ok(ResolvedCall {
                base_url: endpoint.trim_end_matches('/').to_string(),
                api_key: config.baseten_token.clone(),
                model: config.open_weights_model.clone(),
                open_weights: true,
            })
        }
    }
}
