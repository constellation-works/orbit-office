//! Hosted (Meta) calls against the loopback fake server.

use office_common::Sensitivity;
use office_core::ports::{ExtractRequest, ImageRequest, Models, TranscribeRequest};
use serde_json::json;

use super::{FakeServer, chat_reply, client, hosted_config};

fn transcribe_request() -> TranscribeRequest {
    TranscribeRequest {
        audio: b"fake-audio".to_vec(),
        duration_seconds: 1,
        mime: "audio/wav".into(),
        sensitivity: Sensitivity::Normal,
        language: None,
    }
}

#[test]
fn transcribes_audio_on_the_hosted_endpoint() {
    let server = FakeServer::start(200, chat_reply("hello from the meeting"));
    let models = client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "hello from the meeting");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer test-meta-key")
    );
}

#[test]
fn extracts_structured_json_on_the_hosted_endpoint() {
    let server = FakeServer::start(200, chat_reply(r#"{"total":"12.50"}"#));
    let models = client(hosted_config(&server.base_url));
    let value = models
        .extract(&ExtractRequest {
            text: "receipt total 12.50".into(),
            schema: json!({"type": "object"}),
            sensitivity: Sensitivity::Normal,
        })
        .expect("extract");
    assert_eq!(value, json!({"total": "12.50"}));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn reads_an_image_on_the_hosted_endpoint() {
    let server = FakeServer::start(200, chat_reply(r#"{"total":"12.50"}"#));
    let models = client(hosted_config(&server.base_url));
    let reading = models
        .read_image(&ImageRequest {
            image: b"fake-png".to_vec(),
            mime: "image/png".into(),
            prompt: "Read the receipt total as JSON.".into(),
            sensitivity: Sensitivity::Normal,
        })
        .expect("read image");
    assert_eq!(reading.fields, json!({"total": "12.50"}));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn maps_a_server_error_to_a_retryable_typed_error() {
    let server = FakeServer::start(500, "{}".into());
    let models = client(hosted_config(&server.base_url));
    let error = models
        .transcribe(&transcribe_request())
        .expect_err("500 fails");
    assert_eq!(error.code(), "model_unavailable");
    assert!(error.retryable());
}

#[test]
fn maps_a_client_error_to_a_non_retryable_typed_error() {
    let server = FakeServer::start(400, "{}".into());
    let models = client(hosted_config(&server.base_url));
    let error = models
        .transcribe(&transcribe_request())
        .expect_err("400 fails");
    assert_eq!(error.code(), "model_request_rejected");
    assert!(!error.retryable());
}

#[test]
fn refuses_a_non_allowlisted_host_before_any_request() {
    let mut config = hosted_config("https://evil.example.com/v1");
    config.meta_base_url = "https://evil.example.com/v1".into();
    let models = client(config);
    let error = models
        .transcribe(&transcribe_request())
        .expect_err("host refused");
    assert_eq!(error.code(), "model_host_not_allowed");
    assert!(!error.retryable());
}

#[test]
fn never_leaks_the_secret_into_an_error_or_a_log_line() {
    let secret = "sk-test-SECRET-value-12345";
    // A hostile endpoint echoes the credential back in its error body.
    let server = FakeServer::start(401, format!(r#"{{"error":"bad key {secret}"}}"#));
    let mut config = hosted_config(&server.base_url);
    config.meta_api_key = Some(secret.into());
    let models = client(config);

    let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = {
        let logs = std::sync::Arc::clone(&logs);
        move || LogWriter(std::sync::Arc::clone(&logs))
    };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .finish();
    let error = tracing::subscriber::with_default(subscriber, || {
        models
            .transcribe(&transcribe_request())
            .expect_err("401 fails")
    });

    assert_eq!(
        server.requests()[0].authorization.as_deref(),
        Some(format!("Bearer {secret}").as_str())
    );
    assert!(
        !format!("{error}").contains(secret),
        "error message leaked the secret"
    );
    assert!(
        !format!("{error:?}").contains(secret),
        "error debug leaked the secret"
    );
    let captured = String::from_utf8(logs.lock().expect("logs lock").clone()).expect("utf8 logs");
    assert!(
        !captured.contains(secret),
        "log line leaked the secret: {captured}"
    );
}

#[derive(Clone)]
struct LogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("logs lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
