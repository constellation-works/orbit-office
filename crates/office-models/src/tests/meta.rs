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
fn maps_reported_speaker_turns_through_the_models_port() {
    let content = r#"{"text":"Jane opened. Daniel agreed.","segments":[{"speaker":"Jane","start_ms":0,"end_ms":4000,"text":"Jane opened."},{"speaker":null,"start_ms":751000,"end_ms":752500,"text":"Daniel agreed."}]}"#;
    let server = FakeServer::start(200, chat_reply(content));
    let adapter = client(hosted_config(&server.base_url));
    let models: &dyn Models = &adapter;
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "Jane opened. Daniel agreed.");
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("Jane"));
    assert_eq!(transcript.segments[0].start_ms, 0);
    assert_eq!(transcript.segments[0].end_ms, 4000);
    assert_eq!(transcript.segments[0].text, "Jane opened.");
    assert_eq!(transcript.segments[1].speaker, None);
    assert_eq!(transcript.segments[1].start_ms, 751_000);
    assert_eq!(transcript.segments[1].end_ms, 752_500);
    assert_eq!(transcript.segments[1].text, "Daniel agreed.");
    assert_eq!(
        office_core::domain::source::meeting_body(&transcript),
        "[00:00:00] Jane: Jane opened.\n\n[00:12:31] Speaker: Daniel agreed."
    );
    let body: serde_json::Value =
        serde_json::from_str(&server.requests()[0].body).expect("JSON request");
    let instruction = body["messages"][0]["content"][0]["text"]
        .as_str()
        .expect("instruction");
    assert!(
        instruction.contains("segments")
            && instruction.contains("start_ms")
            && instruction.contains("speaker"),
        "transcription must ask for speaker turns and timestamps"
    );
    assert_eq!(body["response_format"]["json_schema"]["name"], "transcript");
}

#[test]
fn keeps_text_and_no_turns_when_the_reply_reports_none() {
    let server = FakeServer::start(200, chat_reply(r#"{"text":"no turns","segments":[]}"#));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "no turns");
    assert!(transcript.segments.is_empty());
    assert_eq!(
        office_core::domain::source::meeting_body(&transcript),
        "no turns"
    );
}

#[test]
fn drops_a_malformed_turn_and_keeps_its_siblings() {
    let content = r#"{"text":"kept","segments":[
        {"speaker":"  Ada  ","start_ms":0,"end_ms":10,"text":"  hello  "},
        {"speaker":"Nope","end_ms":10,"text":"missing start"},
        {"speaker":"Late","start_ms":20,"end_ms":10,"text":"ends before it starts"},
        {"speaker":"","start_ms":30,"end_ms":40,"text":"blank speaker"},
        {"speaker":1,"start_ms":50,"end_ms":60,"text":"bad speaker"}
    ]}"#;
    let server = FakeServer::start(200, chat_reply(content));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "kept");
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("Ada"));
    assert_eq!(transcript.segments[0].text, "hello");
    assert_eq!(transcript.segments[1].speaker, None);
    assert_eq!(transcript.segments[1].text, "blank speaker");
    assert_eq!(transcript.segments[1].start_ms, 30);
}

#[test]
fn reads_speaker_turns_from_a_fenced_json_reply() {
    let content = "```json\n{\"text\":\"hello\",\"segments\":[{\"speaker\":\"Jane\",\"start_ms\":1000,\"end_ms\":2000,\"text\":\"hello\"}]}\n```";
    let server = FakeServer::start(200, chat_reply(content));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "hello");
    assert_eq!(transcript.segments.len(), 1);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("Jane"));
    assert_eq!(transcript.segments[0].start_ms, 1_000);
    assert_eq!(transcript.segments[0].end_ms, 2_000);
}

#[test]
fn transcribes_audio_on_the_hosted_endpoint() {
    let server = FakeServer::start(200, chat_reply("hello from the meeting"));
    let models = client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe");
    assert_eq!(transcript.text, "hello from the meeting");
    assert!(transcript.segments.is_empty());
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
    let reply = json!({"total": "12.50", "items": [{"category": "food"}]});
    let server = FakeServer::start(200, chat_reply(&reply.to_string()));
    let models = client(hosted_config(&server.base_url));
    let value = models
        .extract(&ExtractRequest {
            text: "receipt total 12.50".into(),
            schema: super::receipt_schema(),
            sensitivity: Sensitivity::Normal,
        })
        .expect("extract");
    assert_eq!(value, reply);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn refuses_schema_violations_on_the_hosted_endpoint() {
    super::assert_extraction_refuses_schema_violations(Sensitivity::Normal);
}

#[test]
fn refuses_invalid_or_external_extraction_schemas_before_inference() {
    let directory = tempfile::tempdir().expect("temporary schema directory");
    let schema_file = directory.path().join("schema.json");
    std::fs::write(&schema_file, r#"{"type":"object"}"#).expect("write schema fixture");
    let server = FakeServer::start(200, chat_reply(r#"{"total":"12.50"}"#));
    let mut config = hosted_config(&server.base_url);
    config.open_weights_endpoint = Some(server.base_url.clone());
    let adapter = client(config);
    let models: &dyn Models = &adapter;

    for sensitivity in [Sensitivity::Normal, Sensitivity::Private] {
        for schema in [
            json!({"type": "sensitive-invalid-type"}),
            json!({"$ref": "#/$defs/missing"}),
            json!({"$ref": format!("{}/schema", server.base_url)}),
            json!({"$ref": format!("file://{}", schema_file.display())}),
            json!(17),
        ] {
            let error = models
                .extract(&ExtractRequest {
                    text: "receipt".into(),
                    schema,
                    sensitivity,
                })
                .expect_err("invalid or external schema must be refused before inference");
            assert_eq!(error.code(), "model_invalid_schema");
            assert!(!error.retryable());
            assert!(
                !format!("{error:?}").contains("sensitive-invalid-type"),
                "schema diagnostics must not expose the schema"
            );
        }
    }
    assert!(server.requests().is_empty());
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
