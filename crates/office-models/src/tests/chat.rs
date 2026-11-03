//! Hosted (Meta) calls against the loopback fake server.

use office_common::Sensitivity;
use office_core::ports::{
    ExtractRequest, ImageRequest, Models, TranscribeRequest, TranscriptChunk,
};
use serde_json::json;

use super::{FakeServer, asr_reply, chat_reply, client, hosted_config};

#[test]
fn digest_schema_accepts_the_shared_shape_and_refuses_wrong_claim_types() {
    let valid = json!({"summary":"Fixture","kind_hint":"design-doc","as_of":null,
        "subjects":[{"name":"Ingest","template":"topic","aliases":[],"role":"owner"}],
        "claims":[{"subject":"Ingest","text":"Grounded","cites":["#L1"],"volatile":false},
                  {"subject":"Ingest","text":"To be dropped by extraction","volatile":true}],
        "procedures":[{"title":"Run","inputs":[],"steps":[{"text":"Do it","cites":[]}],"done_when":"Done"}],
        "terms":[],"refs":[]});
    let mut invalid = valid.clone();
    invalid["claims"][0]["volatile"] = json!("wrong type");
    let server = FakeServer::start_replies(vec![
        (200, chat_reply(&valid.to_string())),
        (200, chat_reply(&invalid.to_string())),
    ]);
    let models = client(hosted_config(&server.base_url));
    let request = ExtractRequest {
        tier: office_core::ports::ModelTier::Standard,
        text: "#L1 Evidence".into(),
        schema: office_core::domain::bootstrap::extraction::schema(),
        sensitivity: Sensitivity::Normal,
    };
    assert_eq!(models.extract(&request).unwrap().value, valid);
    assert_eq!(
        models.extract(&request).unwrap_err().code(),
        "model_invalid_response"
    );
    assert_eq!(server.requests().len(), 2);
}

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
    let turns = json!([
        {"speaker": "A", "startMs": 0, "endMs": 4000, "transcript": "Jane opened."},
        {"startMs": 751_000, "transcript": "Daniel agreed."}
    ]);
    let server = FakeServer::start(200, asr_reply("Jane opened. Daniel agreed.", turns));
    let adapter = client(hosted_config(&server.base_url));
    let models: &dyn Models = &adapter;
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe")
        .value;
    assert_eq!(transcript.text, "Jane opened. Daniel agreed.");
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("A"));
    assert_eq!(transcript.segments[0].start_ms, 0);
    assert_eq!(transcript.segments[0].end_ms, 4000);
    assert_eq!(transcript.segments[0].text, "Jane opened.");
    assert_eq!(transcript.segments[1].speaker, None);
    assert_eq!(transcript.segments[1].start_ms, 751_000);
    // No `endMs` and no later turn: the turn ends with the audio, never
    // before it starts.
    assert_eq!(transcript.segments[1].end_ms, 751_000);
    assert_eq!(transcript.segments[1].text, "Daniel agreed.");
    assert_eq!(
        transcript.chunks,
        vec![TranscriptChunk::Turns(transcript.segments.clone())]
    );
}

#[test]
fn keeps_text_and_no_turns_when_the_reply_reports_none() {
    let server = FakeServer::start(200, asr_reply("no turns", json!([])));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe")
        .value;
    assert_eq!(transcript.text, "no turns");
    assert!(transcript.segments.is_empty());
    assert_eq!(
        transcript.chunks,
        vec![TranscriptChunk::Text("no turns".into())]
    );
}

#[test]
fn drops_a_malformed_turn_and_keeps_its_siblings() {
    let turns = json!([
        {"speaker": "  Ada  ", "startMs": 0, "endMs": 10, "transcript": "  hello  "},
        {"speaker": "Nope", "endMs": 10, "transcript": "missing start"},
        {"speaker": "Quiet", "startMs": 15, "transcript": "   "},
        {"speaker": "", "startMs": 30, "transcript": "blank speaker"},
        {"speaker": 1, "startMs": 50, "endMs": 60, "transcript": "bad speaker"}
    ]);
    let server = FakeServer::start(200, asr_reply("kept", turns));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let transcript = models
        .transcribe(&transcribe_request())
        .expect("transcribe")
        .value;
    assert_eq!(transcript.text, "kept");
    assert_eq!(transcript.segments.len(), 3);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("Ada"));
    assert_eq!(transcript.segments[0].text, "hello");
    assert_eq!(transcript.segments[1].speaker, None);
    assert_eq!(transcript.segments[1].text, "blank speaker");
    assert_eq!(transcript.segments[1].start_ms, 30);
    // An open turn ends where the next one starts.
    assert_eq!(transcript.segments[1].end_ms, 50);
    assert_eq!(transcript.segments[2].speaker, None);
}

#[test]
fn refuses_a_reply_without_a_transcript() {
    let server = FakeServer::start(200, chat_reply("chat shape"));
    let models: &dyn Models = &client(hosted_config(&server.base_url));
    let error = models
        .transcribe(&transcribe_request())
        .expect_err("no transcript");
    assert_eq!(error.code(), "model_invalid_response");
}

#[test]
fn reads_speaker_turns_from_a_fenced_json_reply_on_the_private_route() {
    let content = "```json\n{\"text\":\"hello\",\"segments\":[{\"speaker\":\"Jane\",\"start_ms\":1000,\"end_ms\":2000,\"text\":\"hello\"}]}\n```";
    let server = FakeServer::start(200, chat_reply(content));
    let mut config = hosted_config(&server.base_url);
    config.open_weights = Some(super::open_weights(&server.base_url));
    let models: &dyn Models = &client(config);
    let mut request = transcribe_request();
    request.sensitivity = Sensitivity::Private;
    let transcript = models.transcribe(&request).expect("transcribe").value;
    assert_eq!(transcript.text, "hello");
    assert_eq!(transcript.segments.len(), 1);
    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("Jane"));
    assert_eq!(transcript.segments[0].start_ms, 1_000);
    assert_eq!(transcript.segments[0].end_ms, 2_000);
}

#[test]
fn transcribes_audio_on_the_hosted_endpoint() {
    let server = FakeServer::start(200, asr_reply("hello from the meeting", json!([])));
    let models = client(hosted_config(&server.base_url));
    let mut request = transcribe_request();
    request.language = Some("en".into());
    let routed = models.transcribe(&request).expect("transcribe");
    assert_eq!(routed.value.text, "hello from the meeting");
    assert!(routed.value.segments.is_empty());
    assert_eq!(routed.derived_by.model, "muse-voice-transcribe-1.0");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/asr/transcribe");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer test-meta-key")
    );
    let body = &requests[0].body;
    let (_, after_request) = body.split_once("name=\"request\"").expect("request part");
    let json_part = after_request
        .split("\r\n\r\n")
        .nth(1)
        .and_then(|part| part.split("\r\n").next())
        .expect("request JSON");
    let sent: serde_json::Value = serde_json::from_str(json_part).expect("JSON part");
    assert_eq!(
        sent,
        json!({"model": "muse-voice-transcribe-1.0", "mode": "DIARIZATION", "audioEncoding": "WAV", "languageBias": ["en"]})
    );
    assert!(body.contains(
        "name=\"audio\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nfake-audio\r\n--"
    ));
}

#[test]
fn extracts_structured_json_on_the_hosted_endpoint() {
    let reply = json!({"total": "12.50", "items": [{"category": "food"}]});
    let server = FakeServer::start(200, chat_reply(&reply.to_string()));
    let models = client(hosted_config(&server.base_url));
    let value = models
        .extract(&ExtractRequest {
            tier: office_core::ports::ModelTier::Standard,
            text: "receipt total 12.50".into(),
            schema: super::receipt_schema(),
            sensitivity: Sensitivity::Normal,
        })
        .expect("extract")
        .value;
    assert_eq!(value, reply);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn hosted_extraction_sends_a_non_strict_schema_and_still_validates_locally() {
    // The Meta Model API's strict mode refuses any schema whose `required`
    // omits a property, and office's schemas have optional fields.
    let schema = json!({"type": "object", "additionalProperties": false,
        "properties": {"text": {"type": "string"}, "volatile": {"type": "boolean"}},
        "required": ["text"]});
    let server = FakeServer::start_replies(vec![
        (200, chat_reply(r#"{"text":"kept"}"#)),
        (200, chat_reply(r#"{"volatile":true}"#)),
    ]);
    let models = client(hosted_config(&server.base_url));
    let request = ExtractRequest {
        tier: office_core::ports::ModelTier::Standard,
        text: "notes".into(),
        schema: schema.clone(),
        sensitivity: Sensitivity::Normal,
    };
    assert_eq!(
        models.extract(&request).unwrap().value,
        json!({"text": "kept"})
    );
    assert_eq!(
        models.extract(&request).unwrap_err().code(),
        "model_invalid_response"
    );
    let body: serde_json::Value =
        serde_json::from_str(&server.requests()[0].body).expect("JSON request");
    assert_eq!(body["response_format"]["json_schema"]["strict"], false);
    assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
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
    config.open_weights = Some(super::open_weights(&server.base_url));
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
                    tier: office_core::ports::ModelTier::Standard,
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
        .expect("read image")
        .value;
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
    // A hosted endpoint over plain HTTP off loopback is refused even when
    // configured: only the host is configurable, not the transport rule.
    let config = hosted_config("http://evil.example.com/v1");
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
    config.hosted_api_key = Some(secret.into());
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
