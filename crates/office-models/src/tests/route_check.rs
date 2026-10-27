//! `check_private_route` against a fake private endpoint.

use office_common::Sensitivity;
use office_core::domain::endpoint::Capability;
use office_core::ports::{ExtractRequest, Models};
use serde_json::json;

use super::{FakeServer, PRIVATE_MODEL, client, hosted_config, open_weights_with};

fn reply(model: &str, content: &str) -> String {
    json!({"model": model, "choices": [{"message": {"content": content}}]}).to_string()
}

fn accepted(check: &office_core::ports::RouteCheck) -> Vec<Capability> {
    check
        .probes
        .iter()
        .filter(|probe| probe.accepted)
        .map(|probe| probe.capability)
        .collect()
}

#[test]
fn records_exactly_the_capabilities_the_endpoint_accepts() {
    let company = FakeServer::start_replies(vec![
        (200, reply("muse-spark-open", "{}")),
        (200, reply("muse-spark-open", "a dot")),
        (400, "{}".to_string()),
    ]);
    let meta = FakeServer::start(200, reply("muse-spark-1.3", "{}"));
    let mut config = hosted_config(&meta.base_url);
    // Never checked: the check still reaches it, since it is what measures it.
    config.open_weights = Some(open_weights_with(&company.base_url, None));
    let check = client(config).check_private_route().expect("check");

    assert_eq!(accepted(&check), vec![Capability::Text, Capability::Image]);
    let capabilities: Vec<Capability> = check.probes.iter().map(|p| p.capability).collect();
    assert_eq!(capabilities, Capability::ALL.to_vec());
    assert_eq!(
        check.probes[2].error.as_deref(),
        Some("model_request_rejected")
    );
    assert_eq!(check.probes[0].error, None);
    assert!(check.probes[0].elapsed_ms.is_some());
    assert_eq!(check.probes[2].elapsed_ms, None);
    assert_eq!(check.model, PRIVATE_MODEL);
    assert_eq!(check.endpoint, company.base_url);
    assert_eq!(check.reported_model.as_deref(), Some("muse-spark-open"));

    let requests = company.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body.contains(PRIVATE_MODEL));
    assert!(
        requests[1]
            .body
            .contains("data:image/png;base64,iVBORw0KGgo")
    );
    let audio: serde_json::Value = serde_json::from_str(&requests[2].body).expect("json body");
    assert_eq!(
        audio["messages"][0]["content"][1]["input_audio"]["format"],
        json!("wav")
    );
    assert!(
        meta.requests().is_empty(),
        "the check went to the hosted API"
    );
}

#[test]
fn an_endpoint_failing_the_text_request_is_never_routed_to_afterwards() {
    let company = FakeServer::start_replies(vec![
        (500, "{}".to_string()),
        (200, reply("m", "a dot")),
        (200, reply("m", "{}")),
    ]);
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights = Some(open_weights_with(&company.base_url, None));
    let check = client(config.clone()).check_private_route().expect("check");
    let found = accepted(&check);
    assert!(!found.contains(&Capability::Text));
    assert_eq!(check.probes[0].error.as_deref(), Some("model_unavailable"));
    assert_eq!(company.requests().len(), 3);

    // Route with exactly what the check found.
    config.open_weights = Some(open_weights_with(&company.base_url, Some(&found)));
    let error = client(config)
        .extract(&ExtractRequest {
            text: "private minutes".into(),
            schema: json!({"type": "object"}),
            sensitivity: Sensitivity::Private,
        })
        .expect_err("text check failed");
    assert_eq!(error.code(), "private_endpoint_unverified");
    assert_eq!(
        company.requests().len(),
        3,
        "a request followed the failed check"
    );
}

#[test]
fn a_reported_model_that_echoes_the_token_is_dropped() {
    let company = FakeServer::start(200, reply("x-test-open-weights-token-x", "{}"));
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights = Some(open_weights_with(&company.base_url, None));
    let check = client(config).check_private_route().expect("check");
    assert_eq!(check.reported_model, None);
    assert!(!format!("{check:?}").contains("test-open-weights-token"));
}

#[test]
fn refuses_without_an_endpoint() {
    let meta = FakeServer::start(200, reply("m", "{}"));
    let error = client(hosted_config(&meta.base_url))
        .check_private_route()
        .expect_err("no endpoint");
    assert_eq!(error.code(), "private_endpoint_unconfigured");
    assert!(meta.requests().is_empty());
}

#[test]
fn refuses_an_endpoint_outside_the_allowlist_before_any_request() {
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights = Some(open_weights_with("https://api.meta.ai/v1", None));
    let error = client(config).check_private_route().expect_err("meta");
    assert_eq!(error.code(), "model_host_not_allowed");
}
