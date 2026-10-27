//! Open-weights (Baseten) calls against the loopback fake server.

use office_common::Sensitivity;
use office_core::ports::{ExtractRequest, Models, TranscribeRequest};
use serde_json::json;

use super::{FakeServer, chat_reply, client, hosted_config};

#[test]
fn extracts_through_the_open_weights_endpoint() {
    let open = FakeServer::start(200, chat_reply(r#"{"total":"9.99"}"#));
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    let value = models
        .extract(&ExtractRequest {
            text: "private receipt total 9.99".into(),
            schema: json!({"type": "object"}),
            sensitivity: Sensitivity::Private,
        })
        .expect("extract");
    assert_eq!(value, json!({"total": "9.99"}));
    let requests = open.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .body
            .contains("meta-models/muse-spark-open-1026")
    );
}

#[test]
fn transcribes_private_audio_through_the_open_weights_endpoint() {
    let open = FakeServer::start(200, chat_reply("private transcript"));
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    let transcript = models
        .transcribe(&TranscribeRequest {
            audio: b"fake-audio".to_vec(),
            duration_seconds: 1,
            mime: "audio/wav".into(),
            sensitivity: Sensitivity::Private,
            language: None,
        })
        .expect("transcribe");
    assert_eq!(transcript.text, "private transcript");
    assert!(transcript.segments.is_empty());
    let body = &open.requests()[0].body;
    assert!(
        body.contains("segments") && body.contains("start_ms") && body.contains("speaker"),
        "private transcription must ask for speaker turns and timestamps"
    );
    assert_eq!(open.requests().len(), 1);
}

#[test]
fn refuses_a_non_json_extraction_reply_from_the_open_weights_endpoint() {
    // Muse Spark Open does not enforce json_schema, so a prose reply
    // must be refused rather than trusted as structured output.
    let open = FakeServer::start(200, chat_reply("the total is 9.99, probably"));
    let mut config = hosted_config("http://127.0.0.1:1/v1");
    config.open_weights_endpoint = Some(open.base_url.clone());
    let models = client(config);

    let error = models
        .extract(&ExtractRequest {
            text: "private receipt".into(),
            schema: json!({"type": "object"}),
            sensitivity: Sensitivity::Private,
        })
        .expect_err("prose is not structured JSON");
    assert_eq!(error.code(), "model_invalid_response");
    assert!(!error.retryable());
}
