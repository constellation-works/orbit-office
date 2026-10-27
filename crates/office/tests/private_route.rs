//! Through the built binary: the private route's settings block, the
//! endpoint check against a fake loopback endpoint, and the renamed secret
//! (specs/models-and-privacy.md, "The private route"). STD-04: temp roots,
//! a cleared environment, and no network beyond loopback.
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
        Self {
            _dir: dir,
            workspace,
            state,
        }
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.workspace.join("library/.office/config.yaml")).expect("config")
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

    fn configured(&self, endpoint: &str) {
        assert_eq!(self.tool("office.init", json!({}), json!({}))["ok"], true);
        let response = self.tool(
            "office.config-set",
            json!({"key": "open_weights", "value": {
                "endpoint": endpoint,
                "model": "meta-models/Muse-Spark-Open-816B-A42B",
                "kind": "company",
                "label": "Acme's server",
            }}),
            json!({}),
        );
        assert_eq!(response["ok"], true, "{response}");
    }
}

/// One request the fake endpoint received: its `Authorization` header and body.
type Received = Arc<Mutex<Vec<(Option<String>, String)>>>;

/// A loopback endpoint answering request `n` with `statuses[n]` (the last
/// repeats). Serves exactly `count` requests, then stops.
fn fake_endpoint(statuses: &[u16], count: usize) -> (String, Received) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let base = format!(
        "http://127.0.0.1:{}/v1",
        listener.local_addr().unwrap().port()
    );
    let received: Received = Arc::default();
    let statuses = statuses.to_vec();
    let log = Arc::clone(&received);
    std::thread::spawn(move || {
        for index in 0..count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (authorization, body) = read_request(&mut stream);
            log.lock().unwrap().push((authorization, body));
            let status = statuses
                .get(index)
                .or(statuses.last())
                .copied()
                .unwrap_or(200);
            let reply =
                json!({"model": "muse-spark-open", "choices": [{"message": {"content": "{}"}}]})
                    .to_string();
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (base, received)
}

fn read_request(stream: &mut std::net::TcpStream) -> (Option<String>, String) {
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
        header("authorization"),
        String::from_utf8_lossy(&raw[head_end..]).to_string(),
    )
}

#[test]
fn the_check_records_exactly_what_the_endpoint_accepts_and_commits_it() {
    let env = Env::new();
    // Text and image answered; audio rejected.
    let (endpoint, received) = fake_endpoint(&[200, 200, 400], 3);
    env.configured(&endpoint);

    let response = env.tool(
        "office.private-route-check",
        json!({}),
        json!({"open_weights_token": "fixture-token"}),
    );

    assert_eq!(response["ok"], true, "{response}");
    let output = &response["output"];
    assert_eq!(output["capabilities"], json!(["text", "image"]));
    assert_eq!(output["routable"], json!(true));
    assert_eq!(output["route"], json!("private:company"));
    assert_eq!(output["reported_model"], json!("muse-spark-open"));
    assert_eq!(
        output["checks"][2]["error"],
        json!("model_request_rejected")
    );
    assert!(!response.to_string().contains("fixture-token"));
    let received = received.lock().unwrap().clone();
    assert_eq!(received.len(), 3);
    for (authorization, body) in &received {
        assert_eq!(authorization.as_deref(), Some("Bearer fixture-token"));
        assert!(body.contains("Muse-Spark-Open-816B-A42B"));
    }

    let settings =
        office_core::domain::settings::Settings::from_yaml(&env.config()).expect("valid config");
    let route = settings.open_weights.expect("block kept");
    use office_core::domain::endpoint::Capability;
    assert_eq!(
        route.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert!(route.verified_at.is_some(), "the check time is recorded");
}

#[test]
fn an_endpoint_failing_the_text_request_is_recorded_as_not_routable() {
    let env = Env::new();
    let (endpoint, _received) = fake_endpoint(&[500, 200, 200], 3);
    env.configured(&endpoint);

    let output = env.tool("office.private-route-check", json!({}), json!({}))["output"].clone();

    assert_eq!(output["routable"], json!(false));
    assert_eq!(output["capabilities"], json!(["image", "audio"]));
    assert_eq!(output["checks"][0]["error"], json!("model_unavailable"));
    let route = office_core::domain::settings::Settings::from_yaml(&env.config())
        .unwrap()
        .open_weights
        .unwrap();
    assert!(!route.accepts(office_core::domain::endpoint::Capability::Text));
}

#[test]
fn the_check_refuses_without_an_endpoint() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}), json!({}))["ok"], true);
    let response = env.tool("office.private-route-check", json!({}), json!({}));
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "private_endpoint_unconfigured");
}

#[test]
fn config_set_refuses_bad_open_weights_blocks_naming_the_key() {
    let env = Env::new();
    env.configured("https://muse.internal.acme.example/v1");
    let config = env.config();
    for (key, value, named) in [
        ("open_weights.kind", json!("laptop"), "open_weights.kind"),
        (
            "open_weights.endpoint",
            json!("http://muse.internal.acme.example/v1"),
            "open_weights.endpoint",
        ),
        (
            "open_weights.endpoint",
            json!("https://api.meta.ai/v1"),
            "open_weights.endpoint",
        ),
        (
            "open_weights.capabilities",
            json!(["text"]),
            "open_weights.capabilities",
        ),
        (
            "open_weights.verified_at",
            json!("2026-10-27T10:02:00Z"),
            "open_weights.verified_at",
        ),
    ] {
        let response = env.tool(
            "office.config-set",
            json!({"key": key, "value": value}),
            json!({}),
        );
        assert_eq!(response["ok"], false, "{key}");
        assert_eq!(
            response["error"]["code"], "invalid_library_setting",
            "{key}"
        );
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains(named)),
            "{response}"
        );
        assert_eq!(env.config(), config, "{key} changed the config");
    }
}

#[test]
fn unreadable_settings_are_reported_not_as_an_unset_open_weights_block() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}), json!({}))["ok"], true);
    let config_path = env.workspace.join("library/.office/config.yaml");
    let mut config = std::fs::read_to_string(&config_path).expect("config");
    // The pre-batch default's null legacy key loads as an unset route.
    config.push_str("open_weights_endpoint: null\n");
    std::fs::write(&config_path, &config).expect("rewrite config");
    let response = env.tool("office.private-route-check", json!({}), json!({}));
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(
        response["error"]["code"], "private_endpoint_unconfigured",
        "{response}"
    );
    // A configured legacy endpoint is refused, not treated as unset.
    let config = config.replace(
        "open_weights_endpoint: null\n",
        "open_weights_endpoint: \"http://127.0.0.1:9/v1\"\n",
    );
    std::fs::write(&config_path, config).expect("rewrite config");
    let response = env.tool("office.private-route-check", json!({}), json!({}));
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(
        response["error"]["code"], "invalid_library_setting",
        "{response}"
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("open_weights_endpoint")),
        "{response}"
    );
}

#[test]
fn status_reports_the_open_weights_token_by_presence_only() {
    let env = Env::new();
    let response = env.tool(
        "office.status",
        json!({}),
        json!({"open_weights_token": "fixture-token"}),
    );
    assert_eq!(
        response["output"]["secrets"],
        json!({"meta_api_key": "unset", "open_weights_token": "set", "baseten_token": "set"})
    );
    assert!(!response.to_string().contains("fixture-token"));
}
