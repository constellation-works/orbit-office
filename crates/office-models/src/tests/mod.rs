//! Loopback fake-server harness shared by the adapter tests. No test
//! touches the live network: every endpoint points at loopback, `127.0.0.1`
//! or `[::1]` for the IPv6 case.

mod baseten;
mod http;
mod media;
mod meta;
mod routing;
mod transcribe;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::{ModelConfig, RoutedClient};

/// One request the fake server received.
#[derive(Clone, Debug)]
pub struct Recorded {
    /// Request path, for example `/v1/chat/completions`.
    pub path: String,
    /// `Authorization` header value, when present.
    pub authorization: Option<String>,
    /// Raw request body.
    pub body: String,
}

/// A loopback HTTP server answering with a canned status and body.
pub struct FakeServer {
    /// Base URL (`http://127.0.0.1:<port>/v1`, or `http://[::1]:<port>/v1`)
    /// to configure as an endpoint.
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeServer {
    /// Serve `body` with HTTP `status` for every request.
    pub fn start(status: u16, body: String) -> Self {
        Self::serve("127.0.0.1", vec![(status, body)])
    }

    /// Return successive replies, repeating the last for any extra requests.
    pub fn start_replies(replies: Vec<(u16, String)>) -> Self {
        Self::serve("127.0.0.1", replies)
    }

    /// Serve `body` on the IPv6 loopback address `[::1]`. Panics when the
    /// host has no IPv6 loopback, so the test fails rather than passing
    /// without exercising the endpoint.
    pub fn start_ipv6(status: u16, body: String) -> Self {
        Self::serve("[::1]", vec![(status, body)])
    }

    fn serve(host: &str, replies: Vec<(u16, String)>) -> Self {
        assert!(!replies.is_empty(), "fake server needs at least one reply");
        let listener = TcpListener::bind(format!("{host}:0")).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                listener.set_nonblocking(true).expect("nonblocking");
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            let recorded = read_request(&mut stream);
                            let index = {
                                let mut requests = requests.lock().expect("requests lock");
                                let index = requests.len();
                                requests.push(recorded);
                                index
                            };
                            let (status, body) = replies
                                .get(index)
                                .unwrap_or_else(|| replies.last().expect("nonempty replies"));
                            let response = format!(
                                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = stream.write_all(response.as_bytes());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            base_url: format!("http://{host}:{port}/v1"),
            requests,
            stop,
            thread: Some(thread),
        }
    }

    /// Requests received so far.
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> Recorded {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 4096];
    let header_end = loop {
        if let Some(pos) = find(&raw, b"\r\n\r\n") {
            break pos + 4;
        }
        match stream.read(&mut buf) {
            Ok(0) => break raw.len(),
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(_) => break raw.len(),
        }
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_length: usize = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse().ok())
        })
        .unwrap_or(0);
    while raw.len() < header_end + content_length {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    Recorded {
        path: head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string(),
        authorization: head.lines().find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("authorization:")
                .map(|_| {
                    line.split_once(':')
                        .map(|(_, value)| value.trim().to_string())
                        .unwrap_or_default()
                })
        }),
        body: String::from_utf8_lossy(&raw[header_end..]).to_string(),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A Chat Completions reply carrying `content` as the message text.
pub fn chat_reply(content: &str) -> String {
    json!({"choices": [{"message": {"content": content}}]}).to_string()
}

/// Client config pointing the hosted endpoint at `hosted_base`.
pub fn hosted_config(hosted_base: &str) -> ModelConfig {
    let mut config = ModelConfig::new(
        Some("test-meta-key".into()),
        Some("test-baseten-token".into()),
    );
    config.meta_base_url = hosted_base.to_string();
    config
}

/// Build the routed client for `config`.
pub fn client(config: ModelConfig) -> RoutedClient {
    RoutedClient::new(config)
}
