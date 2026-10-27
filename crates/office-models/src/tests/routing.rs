//! Privacy routing against two loopback servers: hosted and open-weights.

use office_common::Sensitivity;
use office_core::ports::{ExtractRequest, Models};
use serde_json::json;

use super::{FakeServer, chat_reply, client, hosted_config};

fn extract(sensitivity: Sensitivity) -> ExtractRequest {
    ExtractRequest {
        text: "sensitive minutes".into(),
        schema: json!({"type": "object"}),
        sensitivity,
    }
}

#[test]
fn sends_private_material_only_to_the_open_weights_endpoint() {
    let hosted = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let open = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&hosted.base_url);
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    let value = models
        .extract(&extract(Sensitivity::Private))
        .expect("extract");
    assert_eq!(value, json!({"ok": true}));
    assert_eq!(open.requests().len(), 1);
    assert_eq!(
        open.requests()[0].authorization.as_deref(),
        Some("Bearer test-baseten-token")
    );
    assert!(
        hosted.requests().is_empty(),
        "private material reached the hosted endpoint"
    );
}

#[test]
fn sends_normal_material_to_the_hosted_endpoint() {
    let hosted = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let open = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&hosted.base_url);
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    models
        .extract(&extract(Sensitivity::Normal))
        .expect("extract");
    assert_eq!(hosted.requests().len(), 1);
    assert!(open.requests().is_empty());
}

#[test]
fn reaches_an_ipv6_loopback_open_weights_endpoint() {
    let open = FakeServer::start_ipv6(200, chat_reply(r#"{"ok":true}"#));
    let hosted = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&hosted.base_url);
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    let value = models
        .extract(&extract(Sensitivity::Private))
        .expect("extract");
    assert_eq!(value, json!({"ok": true}));
    assert_eq!(open.requests().len(), 1);
    assert!(hosted.requests().is_empty());
}

#[test]
fn refuses_private_material_for_a_non_allowlisted_open_weights_host() {
    let mut config = hosted_config("https://api.meta.ai/v1");
    config.open_weights_endpoint = Some("https://lan-inference.example:8080/v1".into());
    let models = client(config);

    let error = models
        .extract(&extract(Sensitivity::Private))
        .expect_err("non-allowlisted open-weights host");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(!error.retryable());
}

#[test]
fn refuses_private_material_aimed_at_the_hosted_meta_host() {
    let mut config = hosted_config("http://127.0.0.1:9/v1");
    config.open_weights_endpoint = Some("https://api.meta.ai/v1".into());
    let models = client(config);

    let error = models
        .extract(&extract(Sensitivity::Private))
        .expect_err("meta is not an open-weights host");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(!error.retryable());
    assert!(!error.to_string().contains("test-meta-key"));
}

#[test]
fn refuses_private_material_when_no_open_weights_endpoint_is_configured() {
    let hosted = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let models = client(hosted_config(&hosted.base_url));

    let error = models
        .extract(&extract(Sensitivity::Private))
        .expect_err("private without an open endpoint is refused");
    assert_eq!(error.code(), "private_endpoint_unconfigured");
    assert!(!error.retryable());
    assert!(
        hosted.requests().is_empty(),
        "refused private material must not fall back to the hosted endpoint"
    );
}
