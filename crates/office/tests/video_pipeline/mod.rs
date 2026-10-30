//! Real exec composition: stub media processes, a local private model, and a temp Library.
use super::{Env, git};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct ModelServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Value>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl ModelServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = Arc::clone(&stop);
        let thread_requests = Arc::clone(&requests);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let (mut socket, _) = listener.accept().unwrap();
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let n = socket.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                    assert!(raw.len() < 1024 * 1024);
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&raw[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_string)
                            })
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        if raw.len() >= end + 4 + length {
                            thread_requests.lock().unwrap().push(
                                serde_json::from_slice(&raw[end + 4..end + 4 + length]).unwrap(),
                            );
                            break;
                        }
                    }
                }
                let body =
                    json!({"choices":[{"message":{"content":"Quasar visible screen text"}}]})
                        .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        Self {
            address,
            stop,
            requests,
            thread: Some(thread),
        }
    }
}
impl Drop for ModelServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn executable(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn media_path(env: &Env) -> String {
    let bin = env.workspace.join("media-bin");
    std::fs::create_dir(&bin).unwrap();
    executable(
        &bin.join("ffprobe"),
        "#!/bin/sh\nprintf '%s' '{\"format\":{\"duration\":\"120\"},\"streams\":[{\"codec_type\":\"video\"},{\"codec_type\":\"audio\"}]}'\n",
    );
    let raw = bin.join("raw");
    let hashes = [0u64, 1, 0xffff, 0xffff_0000, u64::MAX, 0];
    let mut bytes = Vec::new();
    for hash in hashes {
        for row in 0..8 {
            let mut value = 100u8;
            bytes.push(value);
            for col in 0..8 {
                let bit = 63 - (row * 8 + col);
                if hash & (1 << bit) != 0 {
                    value -= 1;
                } else {
                    value += 1;
                }
                bytes.push(value);
            }
        }
    }
    std::fs::write(&raw, bytes).unwrap();
    executable(
        &bin.join("ffmpeg"),
        &format!(
            "#!/bin/sh\nfor arg; do\nif [ \"$arg\" = rawvideo ]; then\nprintf 'frame:0 pts_time:0\\nlavfi.scene_score=0.4\\nframe:1 pts_time:10\\nlavfi.scene_score=0.5\\nframe:2 pts_time:20\\nlavfi.scene_score=0.8\\nframe:3 pts_time:40\\nlavfi.scene_score=0.6\\nframe:4 pts_time:70\\nlavfi.scene_score=0.9\\nframe:5 pts_time:90\\nlavfi.scene_score=0.7\\n' > scenes.txt\nexec /bin/cat '{}'\nfi\ndone\nprintf '\\211PNG\\015\\012\\032\\012fixture'\n",
            raw.display()
        ),
    );
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}
#[test]
fn video_plan_and_frames_persist_and_route_private_images_under_the_source_id() {
    let env = Env::new();
    let initialized = env.tool("office.init", json!({}));
    assert_eq!(initialized["ok"], true, "{initialized}");
    let server = ModelServer::new();
    let mut settings=office_core::domain::settings::Settings::default().with_setting("open_weights",json!({"endpoint":format!("http://{}/v1",server.address),"model":"fixture-private","kind":"own_hardware","label":"Fixture model"})).unwrap();
    settings = settings
        .with_route_check(
            &format!("http://{}/v1", server.address),
            "fixture-private",
            &office_core::domain::endpoint::Capability::ALL,
            "1970-01-01T00:00:00Z",
        )
        .unwrap();
    std::fs::write(
        env.library().join(".office/config.yaml"),
        settings.to_yaml().unwrap(),
    )
    .unwrap();
    let video = env.workspace.join("fixture.mp4");
    std::fs::write(&video, b"fixture video").unwrap();
    let captured = env.tool(
        "office.capture",
        json!({"upload_path":video,"title":"Video fixture"}),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let id = &captured["output"]["id"];
    let path = media_path(&env);
    let preview = env.tool_with_path(
        "office.video-plan",
        json!({"id":id,"sensitivity":"private"}),
        &path,
    );
    assert_eq!(preview["ok"], true, "{preview}");
    assert!(server.requests.lock().unwrap().is_empty());
    let digest = &preview["output"]["plan"]["digest"];
    assert_eq!(
        preview["output"]["plan"]["counts"],
        json!({"audio":1,"frames":4,"extract":1})
    );
    assert!(preview["output"]["plan"]["estimate"].is_null());
    assert_eq!(
        env.tool_with_path("office.frames", json!({"id":id}), &path)["error"]["code"],
        "plan_unconfirmed"
    );
    assert_eq!(
        env.tool_with_path(
            "office.video-plan",
            json!({"id":id,"confirm":true,"digest":"stale"}),
            &path
        )["error"]["code"],
        "plan_changed"
    );
    let confirmed = env.tool_with_path(
        "office.video-plan",
        json!({"id":id,"confirm":true,"digest":digest}),
        &path,
    );
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    let done = env.tool_with_path("office.frames", json!({"id":id}), &path);
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(done["output"]["calls"]["frames"], 4);
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "fixture-private")
    );
    for request in requests {
        assert!(
            request["messages"]
                .to_string()
                .contains("data:image/png;base64,")
        );
    }
    let frames = env.library().join(done["output"]["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(&frames)
            .unwrap()
            .matches("#frame=")
            .count(),
        4
    );
    assert_eq!(
        std::fs::read_dir(frames.parent().unwrap().join("frames"))
            .unwrap()
            .count(),
        4
    );
    let source = env
        .library()
        .join(captured["output"]["path"].as_str().unwrap());
    let markdown = std::fs::read_to_string(source).unwrap();
    let state = office_core::domain::source::video::state(&markdown)
        .unwrap()
        .unwrap();
    assert_eq!(state.frames_read, 4);
    assert_eq!(state.calls.frames, 4);
    assert_eq!(markdown.matches("derived_by:").count(), 1);
    assert!(office_core::domain::source::parse_source_participants(&markdown).is_ok());
    let tracked = git(&env.library(), &["ls-files"]);
    assert!(tracked.lines().any(|line| line.ends_with("/frames.md")));
    assert!(!tracked.lines().any(|line| line.contains("/frames/")));
    let search = env.tool("office.search", json!({"query":"Quasar"}));
    assert_eq!(search["output"]["hits"][0]["id"], *id, "{search}");
    assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
    assert_eq!(
        env.tool("office.search", json!({"query":"Quasar"}))["output"]["hits"][0]["id"],
        *id
    );
    assert_eq!(
        env.tool_with_path("office.frames", json!({"id":id}), &path),
        done
    );
    assert_eq!(server.requests.lock().unwrap().len(), 4);
}
#[test]
fn init_upgrades_gitignore_preserving_every_existing_byte_and_is_then_idempotent() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let path = env.library().join(".gitignore");
    std::fs::write(&path, b"# user policy\r\noriginal.*\r\ncustom-ignore").unwrap();
    let upgraded = env.tool("office.init", json!({}));
    assert_eq!(upgraded["ok"], true, "{upgraded}");
    assert_eq!(upgraded["output"]["already_initialized"], false);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"# user policy\r\noriginal.*\r\ncustom-ignore\nframes/\n.office/state/\n"
    );
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let repeat = env.tool("office.init", json!({}));
    assert_eq!(repeat["output"]["already_initialized"], true);
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
}
