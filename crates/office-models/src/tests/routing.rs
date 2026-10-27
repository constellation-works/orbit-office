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
