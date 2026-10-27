//! HTTP host policy, redirects and IPv6 endpoint handling.

use super::{FakeServer, chat_reply, client, hosted_config, receipt_schema};
use crate::http::{check_url, host_of};
use office_common::Sensitivity;
use office_core::ports::{ExtractRequest, Models};

#[test]
fn does_not_follow_redirect_to_an_unlisted_host() {
    let unlisted = FakeServer::start_unlisted_loopback(200, chat_reply(r#"{"total":"9.99"}"#));
    let redirect = FakeServer::start_redirect(unlisted.base_url.clone());
    let adapter = client(hosted_config(&redirect.base_url));
    let models: &dyn Models = &adapter;
    let result = models.extract(&ExtractRequest {
        text: "receipt".into(),
        schema: receipt_schema(),
        sensitivity: Sensitivity::Normal,
    });

    assert_eq!(redirect.requests().len(), 1);
    assert!(
        unlisted.requests().is_empty(),
        "the redirect target must receive no request"
    );
    assert!(
        result.is_err(),
        "a redirect response is not a model response"
    );
}

#[test]
fn host_of_strips_brackets_and_port_from_an_ipv6_literal() {
    assert_eq!(host_of("http://[::1]:8080/v1").as_deref(), Some("::1"));
    assert_eq!(host_of("http://[::1]/v1").as_deref(), Some("::1"));
    assert_eq!(
        host_of("https://user@[::1]:8443/v1").as_deref(),
        Some("::1")
    );
}

#[test]
fn host_of_refuses_an_unterminated_ipv6_literal() {
    assert_eq!(host_of("http://[::1:8080/v1"), None);
}

#[test]
fn check_url_accepts_an_ipv6_loopback_endpoint_over_http() {
    assert_eq!(
        check_url("http://[::1]:8080/v1", false).expect("loopback"),
        "::1"
    );
}

#[test]
fn check_url_refuses_a_non_loopback_ipv6_host() {
    let error = check_url("http://[2001:db8::1]:8080/v1", false).expect_err("not allowlisted");
    assert_eq!(error.code(), "model_host_not_allowed");
}

#[test]
fn check_url_accepts_meta_and_baseten_over_https_only() {
    assert_eq!(
        check_url("https://api.meta.ai/v1", false).expect("meta"),
        "api.meta.ai"
    );
    assert_eq!(
        check_url("https://inference.baseten.co/v1", false).expect("baseten"),
        "inference.baseten.co"
    );
    assert_eq!(
        check_url("https://model-abc.api.baseten.co/v1", false).expect("deployment"),
        "model-abc.api.baseten.co"
    );
    assert_eq!(
        check_url("http://inference.baseten.co/v1", false)
            .expect_err("https required")
            .code(),
        "model_host_not_allowed"
    );
    assert_eq!(
        check_url("https://lan-inference.example/v1", false)
            .expect_err("not allowlisted")
            .code(),
        "model_host_not_allowed"
    );
}

#[test]
fn check_url_keeps_open_weights_calls_on_baseten_or_loopback() {
    let error = check_url("https://api.meta.ai/v1", true).expect_err("meta is hosted-only");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert_eq!(
        check_url("https://inference.baseten.co/v1", true).expect("baseten"),
        "inference.baseten.co"
    );
    assert_eq!(
        check_url("http://127.0.0.1:9/v1", true).expect("loopback"),
        "127.0.0.1"
    );
}
