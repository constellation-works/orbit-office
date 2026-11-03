//! HTTP host policy, redirects and IPv6 endpoint handling.

use super::{FakeServer, chat_reply, client, hosted_config, open_weights, receipt_schema};
use crate::http::{Destination, check_url, host_of, post_json};
use office_common::Sensitivity;
use office_core::ports::{ExtractRequest, Models};
use serde_json::json;

const COMPANY: &str = "https://muse.internal.acme.example/v1";

fn hosted_to(endpoint: &str) -> Destination<'_> {
    Destination::Hosted { endpoint }
}

fn private_to(endpoint: &str) -> Destination<'_> {
    Destination::Private { endpoint }
}

#[test]
fn does_not_follow_redirect_to_an_unlisted_host() {
    let unlisted = FakeServer::start_unlisted_loopback(200, chat_reply(r#"{"total":"9.99"}"#));
    let redirect = FakeServer::start_redirect(unlisted.base_url.clone());
    let adapter = client(hosted_config(&redirect.base_url));
    let models: &dyn Models = &adapter;
    let result = models.extract(&ExtractRequest {
        tier: office_core::ports::ModelTier::Standard,
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
fn a_private_endpoint_redirect_never_reaches_its_target() {
    let target = FakeServer::start(200, chat_reply(r#"{"total":"9.99"}"#));
    let company = FakeServer::start_redirect(format!("{}/chat/completions", target.base_url));
    let hosted = FakeServer::start(200, chat_reply(r#"{"total":"9.99"}"#));
    let mut config = hosted_config(&hosted.base_url);
    config.open_weights = Some(open_weights(&company.base_url));
    let models = client(config);

    let result = models.extract(&ExtractRequest {
        tier: office_core::ports::ModelTier::Standard,
        text: "private receipt".into(),
        schema: receipt_schema(),
        sensitivity: Sensitivity::Private,
    });

    assert!(result.is_err(), "a redirect is not a model response");
    assert_eq!(company.requests().len(), 1);
    assert!(
        target.requests().is_empty(),
        "redirect target was contacted"
    );
    assert!(hosted.requests().is_empty(), "private material went hosted");
}

#[test]
fn a_private_post_to_another_host_is_refused_before_a_socket_opens() {
    let configured = FakeServer::start(200, chat_reply("{}"));
    let other = FakeServer::start(200, chat_reply("{}"));
    let agent = ureq::Agent::new_with_defaults();

    let error = post_json(
        &agent,
        &format!("{}/chat/completions", other.base_url),
        Some("token"),
        &json!({}),
        private_to(&configured.base_url),
        None,
    )
    .expect_err("another loopback port is another origin");

    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(other.requests().is_empty(), "the other host was contacted");
    assert!(configured.requests().is_empty());
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
        check_url("http://[::1]:8080/v1", hosted_to("http://[::1]:8080/v1")).expect("loopback"),
        "::1"
    );
    assert_eq!(
        check_url(
            "http://[::1]:8080/v1/chat/completions",
            private_to("http://[::1]:8080/v1")
        )
        .expect("configured loopback"),
        "::1"
    );
}

#[test]
fn check_url_refuses_a_non_loopback_ipv6_host() {
    let error = check_url(
        "http://[2001:db8::1]:8080/v1",
        hosted_to("http://[2001:db8::1]:8080/v1"),
    )
    .expect_err("not allowlisted");
    assert_eq!(error.code(), "model_host_not_allowed");
}

#[test]
fn hosted_calls_reach_the_default_meta_api_over_https() {
    let meta = office_core::domain::endpoint::DEFAULT_HOSTED_ENDPOINT;
    assert_eq!(
        check_url("https://api.meta.ai/v1/chat/completions", hosted_to(meta)).expect("meta"),
        "api.meta.ai"
    );
    for url in [
        "http://api.meta.ai/v1",
        "https://inference.baseten.co/v1",
        "https://model-abc.api.baseten.co/v1",
        "https://muse.internal.acme.example/v1",
        "https://user:pw@api.meta.ai/v1/chat/completions",
    ] {
        assert_eq!(
            check_url(url, hosted_to(meta))
                .expect_err("not the hosted API")
                .code(),
            "model_host_not_allowed",
            "{url}"
        );
    }
}

#[test]
fn hosted_calls_reach_only_the_configured_hosted_endpoints_exact_origin() {
    let configured = "https://api.example-llm.com/v1";
    assert_eq!(
        check_url(
            "https://api.example-llm.com/v1/chat/completions",
            hosted_to(configured)
        )
        .expect("configured hosted endpoint"),
        "api.example-llm.com"
    );
    for url in [
        "https://api.meta.ai/v1/chat/completions",
        "https://api.example-llm.com:8443/v1/chat/completions",
        "http://api.example-llm.com/v1/chat/completions",
        "https://api.example-llm.com.evil.example/v1/chat/completions",
        "https://user:pw@api.example-llm.com/v1/chat/completions",
    ] {
        assert_eq!(
            check_url(url, hosted_to(configured))
                .expect_err("not the configured hosted origin")
                .code(),
            "model_host_not_allowed",
            "{url}"
        );
    }
}

#[test]
fn a_hosted_endpoint_off_https_or_with_credentials_reaches_nothing() {
    for endpoint in [
        "http://api.example-llm.com/v1",
        "https://user:pw@api.example-llm.com/v1",
        "https://api.example-llm.com/v1?key=x",
        "ftp://api.example-llm.com/v1",
    ] {
        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
        assert_eq!(
            check_url(&url, hosted_to(endpoint))
                .expect_err("endpoint refused")
                .code(),
            "model_host_not_allowed",
            "{endpoint}"
        );
    }
}

#[test]
fn private_calls_reach_only_the_configured_endpoints_exact_origin() {
    assert_eq!(
        check_url(
            "https://muse.internal.acme.example/v1/chat/completions",
            private_to(COMPANY)
        )
        .expect("configured company host"),
        "muse.internal.acme.example"
    );
    for url in [
        "https://inference.baseten.co/v1/chat/completions",
        "https://model-abc.api.baseten.co/v1/chat/completions",
        "https://api.meta.ai/v1/chat/completions",
        "https://muse.internal.acme.example:8443/v1/chat/completions",
        "http://muse.internal.acme.example/v1/chat/completions",
        "https://other.acme.example/v1/chat/completions",
        "https://muse.internal.acme.example.evil.example/v1/chat/completions",
        "https://evil.example@muse.internal.acme.example.evil.example/v1",
    ] {
        let error = check_url(url, private_to(COMPANY)).expect_err("not the configured host");
        assert_eq!(error.code(), "model_host_not_allowed", "{url}");
        assert!(!error.retryable());
    }
}

#[test]
fn a_configured_baseten_host_does_not_admit_other_baseten_hosts() {
    let provider = "https://model-abc.api.baseten.co/v1";
    assert!(
        check_url(
            "https://model-abc.api.baseten.co/v1/chat/completions",
            private_to(provider)
        )
        .is_ok()
    );
    for url in [
        "https://inference.baseten.co/v1/chat/completions",
        "https://model-xyz.api.baseten.co/v1/chat/completions",
    ] {
        assert_eq!(
            check_url(url, private_to(provider))
                .expect_err("only the configured Baseten host")
                .code(),
            "model_host_not_allowed"
        );
    }
}

#[test]
fn private_calls_need_https_off_loopback_even_when_configured() {
    let error = check_url(
        "http://muse.internal.acme.example/v1/chat/completions",
        private_to("http://muse.internal.acme.example/v1"),
    )
    .expect_err("plain http off loopback");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(error.to_string().contains("HTTPS"));
    assert!(
        check_url(
            "http://127.0.0.1:9/v1/chat/completions",
            private_to("http://127.0.0.1:9/v1")
        )
        .is_ok()
    );
}

#[test]
fn private_calls_never_reach_the_meta_api_even_when_configured() {
    let error = check_url(
        "https://api.meta.ai/v1/chat/completions",
        private_to("https://api.meta.ai/v1"),
    )
    .expect_err("meta is never private");
    assert_eq!(error.code(), "model_host_not_allowed");
}
