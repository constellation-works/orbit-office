//! Privacy routing against two loopback servers: a hosted fake standing in
//! for `api.meta.ai` and a company private endpoint.

use office_common::Sensitivity;
use office_core::domain::endpoint::{Capability, EndpointKind};
use office_core::ports::{ExtractRequest, ImageRequest, Models, TranscribeRequest};
use serde_json::json;

use super::{
    FakeServer, PRIVATE_MODEL, chat_reply, client, hosted_config, open_weights, open_weights_with,
};

fn extract(sensitivity: Sensitivity) -> ExtractRequest {
    ExtractRequest {
        text: "sensitive minutes".into(),
        schema: json!({"type": "object"}),
        sensitivity,
    }
}

fn image(sensitivity: Sensitivity) -> ImageRequest {
    ImageRequest {
        image: b"fake-png".to_vec(),
        mime: "image/png".into(),
        prompt: "Read the receipt total as JSON.".into(),
        sensitivity,
    }
}

fn audio(sensitivity: Sensitivity) -> TranscribeRequest {
    TranscribeRequest {
        audio: b"fake-audio".to_vec(),
        duration_seconds: 1,
        mime: "audio/wav".into(),
        sensitivity,
        language: None,
    }
}

/// Each private operation's route, model and the first request it sent.
fn every_private_operation(models: &dyn Models) -> Vec<(String, String)> {
    let extracted = models
        .extract(&extract(Sensitivity::Private))
        .expect("private extract");
    let read = models
        .read_image(&image(Sensitivity::Private))
        .expect("private image read");
    let transcribed = models
        .transcribe(&audio(Sensitivity::Private))
        .expect("private transcription");
    [
        extracted.derived_by,
        read.derived_by,
        transcribed.derived_by,
    ]
    .into_iter()
    .map(|derived| (derived.route.to_string(), derived.model))
    .collect()
}

#[test]
fn every_private_operation_reaches_the_configured_company_endpoint() {
    let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let company = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&meta.base_url);
    config.open_weights = Some(open_weights(&company.base_url));
    let models = client(config);

    let routes = every_private_operation(&models);

    let expected = ("private:company".to_string(), PRIVATE_MODEL.to_string());
    assert_eq!(routes, vec![expected.clone(), expected.clone(), expected]);
    let requests = company.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer test-open-weights-token")
        );
        assert!(request.body.contains(PRIVATE_MODEL));
    }
    assert!(requests[1].body.contains("image_url"));
    assert!(requests[2].body.contains("input_audio"));
    assert!(
        meta.requests().is_empty(),
        "private material reached the hosted API"
    );
}

#[test]
fn the_route_names_the_configured_endpoint_kind() {
    for (kind, route) in [
        (EndpointKind::Provider, "private:provider"),
        (EndpointKind::OwnHardware, "private:own_hardware"),
    ] {
        let company = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
        let mut config = hosted_config("http://127.0.0.1:1/v1");
        let mut block = open_weights(&company.base_url);
        block.kind = kind;
        config.open_weights = Some(block);
        let derived = client(config)
            .extract(&extract(Sensitivity::Private))
            .expect("extract")
            .derived_by;
        assert_eq!(derived.route.to_string(), route);
    }
}

#[test]
fn sends_normal_material_to_the_hosted_endpoint_and_records_hosted() {
    let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let company = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&meta.base_url);
    config.open_weights = Some(open_weights(&company.base_url));
    let models = client(config);

    let routed = models
        .extract(&extract(Sensitivity::Normal))
        .expect("extract");
    assert_eq!(routed.derived_by.route.to_string(), "hosted");
    assert_eq!(routed.derived_by.model, crate::DEFAULT_HOSTED_MODEL);
    assert_eq!(meta.requests().len(), 1);
    assert!(company.requests().is_empty());
}

#[test]
fn reaches_an_ipv6_loopback_private_endpoint() {
    let company = FakeServer::start_ipv6(200, chat_reply(r#"{"ok":true}"#));
    let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&meta.base_url);
    config.open_weights = Some(open_weights(&company.base_url));
    let models = client(config);

    let value = models
        .extract(&extract(Sensitivity::Private))
        .expect("extract")
        .value;
    assert_eq!(value, json!({"ok": true}));
    assert_eq!(company.requests().len(), 1);
    assert!(meta.requests().is_empty());
}

#[test]
fn every_private_operation_is_refused_without_an_endpoint_and_nothing_goes_hosted() {
    let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let models = client(hosted_config(&meta.base_url));

    let errors = [
        models.extract(&extract(Sensitivity::Private)).map(|_| ()),
        models.read_image(&image(Sensitivity::Private)).map(|_| ()),
        models.transcribe(&audio(Sensitivity::Private)).map(|_| ()),
    ];
    for result in errors {
        let error = result.expect_err("private without an endpoint is refused");
        assert_eq!(error.code(), "private_endpoint_unconfigured");
        assert!(!error.retryable());
    }
    assert!(
        meta.requests().is_empty(),
        "refused private material must not fall back to the hosted endpoint"
    );
}

#[test]
fn an_endpoint_that_has_not_passed_the_text_check_is_never_routed_to() {
    for capabilities in [
        None,
        Some(&[][..]),
        Some(&[Capability::Image, Capability::Audio][..]),
    ] {
        let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
        let company = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
        let mut config = hosted_config(&meta.base_url);
        config.open_weights = Some(open_weights_with(&company.base_url, capabilities));
        let models = client(config);

        for result in [
            models.extract(&extract(Sensitivity::Private)).map(|_| ()),
            models.read_image(&image(Sensitivity::Private)).map(|_| ()),
            models.transcribe(&audio(Sensitivity::Private)).map(|_| ()),
        ] {
            let error = result.expect_err("unverified endpoint");
            assert_eq!(
                error.code(),
                "private_endpoint_unverified",
                "{capabilities:?}"
            );
        }
        assert!(
            company.requests().is_empty(),
            "unverified endpoint was contacted"
        );
        assert!(meta.requests().is_empty());
    }
}

#[test]
fn a_missing_modality_is_refused_by_name_before_any_request() {
    let meta = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let company = FakeServer::start(200, chat_reply(r#"{"ok":true}"#));
    let mut config = hosted_config(&meta.base_url);
    config.open_weights = Some(open_weights_with(
        &company.base_url,
        Some(&[Capability::Text]),
    ));
    let models = client(config);

    let image_error = models
        .read_image(&image(Sensitivity::Private))
        .expect_err("no image capability");
    assert_eq!(image_error.code(), "private_media_unsupported");
    assert!(image_error.to_string().contains("needs: image"));
    let audio_error = models
        .transcribe(&audio(Sensitivity::Private))
        .expect_err("no audio capability");
    assert_eq!(audio_error.code(), "private_media_unsupported");
    assert!(audio_error.to_string().contains("needs: audio"));
    assert!(company.requests().is_empty());

    models
        .extract(&extract(Sensitivity::Private))
        .expect("text still routes");
    assert_eq!(company.requests().len(), 1);
    assert!(meta.requests().is_empty());
}

#[test]
fn refuses_private_material_aimed_at_the_hosted_meta_host() {
    let mut config = hosted_config("http://127.0.0.1:9/v1");
    config.open_weights = Some(open_weights("https://api.meta.ai/v1"));
    let models = client(config);

    let error = models
        .extract(&extract(Sensitivity::Private))
        .expect_err("meta is never the private endpoint");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(!error.retryable());
    assert!(!error.to_string().contains("test-meta-key"));
}

#[test]
fn refuses_a_plain_http_private_endpoint_off_loopback() {
    let mut config = hosted_config("http://127.0.0.1:9/v1");
    config.open_weights = Some(open_weights("http://muse.internal.acme.example/v1"));
    let models = client(config);

    let error = models
        .extract(&extract(Sensitivity::Private))
        .expect_err("plain http off loopback");
    assert_eq!(error.code(), "model_host_not_allowed");
}
