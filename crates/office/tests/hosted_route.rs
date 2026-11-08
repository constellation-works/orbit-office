//! Through the built binary: the hosted route's configurable endpoint
//! (`models.endpoint`, `models.standard`), its endpoint rule, and the
//! unchanged Meta default (specs/models-and-privacy.md, "The hosted
//! route"). STD-04: temp roots, a cleared environment, and no network
//! beyond loopback.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_office");

struct Env {
    _dir: tempfile::TempDir,
    workspace: PathBuf,
    state: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = dir.path().join("plugin_state");
        let env = Self {
            _dir: dir,
            workspace,
            state,
        };
        assert_eq!(env.tool("office.init", json!({}), json!({}))["ok"], true);
        env
    }

    fn library(&self) -> PathBuf {
        self.workspace.join("library")
    }

    fn tool(&self, tool: &str, input: Value, secrets: Value) -> Value {
        let request = json!({
            "schema_version": 1,
            "tool": tool,
            "input": input,
            "context": {
                "workspace_root": self.workspace,
                "plugin_state": self.state,
                "secrets": secrets,
            }
        })
        .to_string();
        let mut child = Command::new(BIN)
            .args(["orbit-tool"])
            .env_clear()
            .env("HOME", self.workspace.join("home"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(request.as_bytes())
            .expect("write request");
        let out = child.wait_with_output().expect("wait");
        assert_eq!(out.status.code(), Some(0));
        serde_json::from_slice(&out.stdout).expect("one JSON document")
    }

    fn set(&self, key: &str, value: Value) -> Value {
        self.tool(
            "office.config-set",
            json!({"key": key, "value": value}),
            json!({}),
        )
    }

    /// One normal-sensitivity document source, indexed.
    fn source(&self) {
        let path = self.library().join("sources/1970/01/src-doc/source.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "---\nid: src-doc\nkind: doc\ntitle: Design\nsensitivity: normal\n---\nEvidence.\n",
        )
        .unwrap();
        let reindexed = self.tool("office.reindex", json!({}), json!({}));
        assert_eq!(reindexed["ok"], true, "{reindexed}");
    }
}

/// One request the fake endpoint received: its path, `Authorization`
/// header and body.
type Received = Arc<Mutex<Vec<(String, Option<String>, String)>>>;

/// A loopback OpenAI-compatible endpoint answering one Chat Completions
/// request with `content`.
fn fake_endpoint(content: &Value) -> (String, Received) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let base = format!(
        "http://127.0.0.1:{}/v1",
        listener.local_addr().unwrap().port()
    );
    let received: Received = Arc::default();
    let log = Arc::clone(&received);
    let reply = json!({"choices": [{"message": {"content": content.to_string()}}]}).to_string();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        log.lock().unwrap().push(read_request(&mut stream));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (base, received)
}

fn read_request(stream: &mut std::net::TcpStream) -> (String, Option<String>, String) {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 8192];
    let head_end = loop {
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break raw.len(),
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let length: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while raw.len() < head_end + length {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    }
    (
        path,
        header("authorization"),
        String::from_utf8_lossy(&raw[head_end..]).to_string(),
    )
}

fn digest_reply() -> Value {
    json!({"summary": "Digest", "kind_hint": "design-doc", "as_of": null,
        "subjects": [{"name": "HostedSubject", "template": "topic", "aliases": []}],
        "claims": [{"subject": "HostedSubject", "text": "Grounded", "cites": ["#L1"], "volatile": false}],
        "procedures": [], "terms": [], "refs": []})
}

#[test]
fn normal_material_goes_to_the_hosted_endpoint_and_model_configured_in_settings() {
    let env = Env::new();
    env.source();
    let (base, received) = fake_endpoint(&digest_reply());
    for (key, value) in [
        ("models.endpoint", json!(base)),
        ("models.standard", json!("acme-chat-large")),
    ] {
        let response = env.set(key, value);
        assert_eq!(response["ok"], true, "{key}: {response}");
    }

    // The key as a save for this endpoint stores it: bound to its origin.
    let origin = base.trim_end_matches("/v1").to_string();
    let stored = json!({"origin": origin, "key": "fixture-hosted-key"}).to_string();
    let extracted = env.tool(
        "office.extract",
        json!({"id": "src-doc", "profile": "digest"}),
        json!({"meta_api_key": stored}),
    );
    assert_eq!(extracted["ok"], true, "{extracted}");
    assert!(!extracted.to_string().contains("fixture-hosted-key"));

    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    let (path, authorization, body) = &received[0];
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(authorization.as_deref(), Some("Bearer fixture-hosted-key"));
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["model"], "acme-chat-large");
}

#[test]
fn a_hosted_key_saved_for_another_origin_is_refused_through_the_binary_and_nothing_is_sent() {
    let env = Env::new();
    env.source();
    let (base, received) = fake_endpoint(&digest_reply());
    let response = env.set("models.endpoint", json!(base));
    assert_eq!(response["ok"], true, "{response}");

    // The key was saved for a different origin than the configured one.
    let stored =
        json!({"origin": "https://other.example", "key": "fixture-hosted-key"}).to_string();
    let extracted = env.tool(
        "office.extract",
        json!({"id": "src-doc", "profile": "digest"}),
        json!({"meta_api_key": stored}),
    );
    assert_eq!(extracted["ok"], false, "{extracted}");
    assert_eq!(extracted["error"]["code"], "hosted_key_other_endpoint");
    assert!(!extracted.to_string().contains("fixture-hosted-key"));
    assert!(
        received.lock().unwrap().is_empty(),
        "no request may carry the key to another origin"
    );

    let status = env.tool("office.status", json!({}), json!({"meta_api_key": stored}));
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["output"]["hosted_key_binding"], "other_endpoint");
    assert_eq!(
        status["output"]["hosted_key_bound_origin"],
        "https://other.example"
    );
    assert!(!status.to_string().contains("fixture-hosted-key"));
}

#[test]
fn config_set_accepts_any_https_hosted_endpoint_and_keeps_the_transport_rule() {
    let env = Env::new();
    for endpoint in [
        "https://api.example-llm.com/v1",
        "https://api.meta.ai/v1",
        "http://127.0.0.1:8080/v1",
    ] {
        let response = env.set("models.endpoint", json!(endpoint));
        assert_eq!(response["ok"], true, "{endpoint}: {response}");
        let config = std::fs::read_to_string(env.library().join(".office/config.yaml")).unwrap();
        assert!(config.contains(endpoint), "{config}");
    }
    for (endpoint, reason) in [
        (
            "http://api.example-llm.com/v1",
            "expected an HTTPS URL (plain HTTP only on loopback)",
        ),
        (
            "https://user:secret-in-url@api.example-llm.com/v1",
            "expected an HTTP(S) base URL with a host, no credentials, query or fragment",
        ),
        (
            "https://api.example-llm.com/v1?key=secret-in-url",
            "expected an HTTP(S) base URL with a host, no credentials, query or fragment",
        ),
    ] {
        let response = env.set("models.endpoint", json!(endpoint));
        assert_eq!(response["ok"], false, "{endpoint}: {response}");
        assert_eq!(
            response["error"]["code"], "invalid_library_setting",
            "{response}"
        );
        let text = response.to_string();
        assert!(text.contains("models.endpoint"), "{text}");
        assert!(text.contains(reason), "{text}");
        assert!(!text.contains("secret-in-url"), "{text}");
    }
    // Clearing the key restores Meta's Model API, the default.
    let cleared = env.set("models.endpoint", Value::Null);
    assert_eq!(cleared["ok"], true, "{cleared}");
    let config = std::fs::read_to_string(env.library().join(".office/config.yaml")).unwrap();
    assert!(!config.contains("\"endpoint\":\"http"), "{config}");
}

#[test]
fn a_config_naming_the_meta_endpoint_and_muse_models_keeps_working() {
    let env = Env::new();
    let path = env.library().join(".office/config.yaml");
    let config = std::fs::read_to_string(&path).unwrap();
    // The bundled default names Muse Wonder and no endpoint: it still loads,
    // and an explicit Meta endpoint with Muse models is accepted unchanged.
    assert!(config.contains("careful: muse-wonder"), "{config}");
    for (key, value) in [
        ("models.endpoint", "https://api.meta.ai/v1"),
        ("models.standard", "muse-spark-1.3"),
        ("models.transcribe", "muse-voice-transcribe-1.0"),
        ("models.careful", "muse-wonder"),
    ] {
        let response = env.set(key, json!(value));
        assert_eq!(response["ok"], true, "{key}: {response}");
    }
    let status = env.tool("office.status", json!({}), json!({}));
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(
        status["output"]["hosted"],
        json!({
            "endpoint": "https://api.meta.ai/v1",
            "standard": "muse-spark-1.3",
            "careful": "muse-wonder",
            "transcribe": "muse-voice-transcribe-1.0",
        })
    );
}
