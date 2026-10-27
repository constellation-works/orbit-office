//! Public dispatch with the real store, a stub ffmpeg, and an offline Models
//! implementation returning no timestamps. No environment changes or live APIs.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use office_core::domain::endpoint::ModelRoute;
use office_core::domain::source::video::VideoAnalysis;
use office_core::ports::{
    CaptureSourceRequest, Clock, DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models,
    Orbit, Ports, RouteCheck, Routed, ScriptRunner, TranscribeRequest, Transcript,
};
use office_core::{Error, Result};
use office_models::media::VideoProcessor;
use office_store::LibraryStore;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;

struct LocalModels {
    processor: VideoProcessor,
    requests: Rc<RefCell<Vec<TranscribeRequest>>>,
}
impl Models for LocalModels {
    fn analyze_video(
        &self,
        _bytes: &[u8],
        _threshold: &serde_json::Number,
    ) -> Result<VideoAnalysis> {
        Ok(VideoAnalysis {
            duration_ms: 601_000,
            has_audio: true,
            candidates: vec![],
        })
    }
    fn media_audio(&self, bytes: &[u8], start: u64, length: u64) -> Result<Vec<u8>> {
        self.processor.audio(bytes, start, length)
    }
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Routed<Transcript>> {
        self.requests.borrow_mut().push(request.clone());
        Ok(Routed {
            value: Transcript {
                text: "Ada: First\nBen: Second".into(),
                ..Transcript::default()
            },
            derived_by: DerivedBy {
                route: ModelRoute::Hosted,
                model: "offline-transcribe".into(),
            },
        })
    }
    fn extract(&self, _request: &ExtractRequest) -> Result<Routed<Value>> {
        Err(Error::OrbitUnavailable)
    }
    fn read_image(&self, _request: &ImageRequest) -> Result<Routed<ImageReading>> {
        Err(Error::OrbitUnavailable)
    }
    fn check_private_route(&self) -> Result<RouteCheck> {
        Err(Error::OrbitUnavailable)
    }
}
struct Unused;
impl Orbit for Unused {}
impl ScriptRunner for Unused {}
impl Clock for Unused {
    fn now(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH
    }
}
fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let raw = json!({"schema_version":1,"tool":tool,"input":input,"context":{}}).to_string();
    serde_json::from_str(&office_tools::serve(
        &office_core::protocol::parse(&raw).unwrap(),
        ports,
    ))
    .unwrap()
}
#[test]
fn stub_ffmpeg_and_timestamp_free_models_publish_anchorable_video_versions_with_a_hard_cap() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let library = workspace.join("library");
    let state = root.path().join("state");
    LibraryStore::initialize_at(&library, &state, None, &workspace).unwrap();
    let store = LibraryStore::open(&library, &state).unwrap();
    let upload = workspace.join("video.mp4");
    std::fs::write(&upload, b"fixture video").unwrap();
    let source = store
        .capture_source(&CaptureSourceRequest {
            upload_path: Some(upload.to_string_lossy().into()),
            captured_at: "1970-01-01T00:00:00Z".into(),
            title: Some("Video".into()),
            ..CaptureSourceRequest::default()
        })
        .unwrap();
    let ffmpeg = root.path().join("ffmpeg");
    // Assert the real adapter supplied offset, length, staged input and WAV output.
    std::fs::write(&ffmpeg,"#!/bin/sh\nset -eu\n[ \"$4\" = '-ss' ] || exit 11\n[ -f \"$7\" ] || exit 12\n[ \"$8\" = '-t' ] || exit 13\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 14\n[ -z \"${META_API_KEY:-}\" ] || exit 15\nprintf 'RIFFxxxxWAVE%s' \"$5\"\n").unwrap();
    std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o700)).unwrap();
    let requests = Rc::new(RefCell::new(vec![]));
    let ports = Ports {
        library: Box::new(store),
        models: Box::new(LocalModels {
            processor: VideoProcessor {
                ffmpeg,
                ffprobe: "/nonexistent".into(),
            },
            requests: requests.clone(),
        }),
        orbit: Box::new(Unused),
        scripts: Box::new(Unused),
        clock: Box::new(Unused),
    };
    let preview = call(&ports, "office.video-plan", json!({"id":source.id}));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["output"]["plan"]["counts"]["audio"], 2);
    let confirmed = call(
        &ports,
        "office.video-plan",
        json!({"id":source.id,"confirm":true,"digest":preview["output"]["plan"]["digest"]}),
    );
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    let result = call(&ports, "office.transcribe", json!({"id":source.id}));
    assert_eq!(result["ok"], true, "{result}");
    let captured = requests.borrow();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].audio, b"RIFFxxxxWAVE0");
    assert_eq!(captured[1].audio, b"RIFFxxxxWAVE600");
    assert_eq!(captured[0].duration_seconds, 600);
    assert_eq!(captured[1].duration_seconds, 1);
    drop(captured);
    let shown = call(
        &ports,
        "office.source-show",
        json!({"id":source.id,"anchor":"#t=00:10:00"}),
    );
    assert_eq!(shown["ok"], true, "{shown}");
    assert!(
        shown["output"]["body"]
            .as_str()
            .unwrap()
            .contains("[00:10:00] Ada: First")
    );
    let markdown = std::fs::read_to_string(library.join(&source.path)).unwrap();
    assert!(markdown.contains("offline-transcribe"));
    let parent = library.join(&source.path).parent().unwrap().to_path_buf();
    assert!(parent.join("v1.md").is_file());
    assert!(parent.join("v2.md").is_file());
    assert_eq!(
        call(&ports, "office.transcribe", json!({"id":source.id}))["error"]["code"],
        "plan_exceeded"
    );
    assert_eq!(requests.borrow().len(), 2);
}
