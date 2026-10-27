//! Video tools through public dispatch, with explicit local-media and model fakes.
use office_core::domain::endpoint::{Capability, EndpointKind};
use office_core::domain::settings::{OpenWeights, Settings};
use office_core::domain::source::{
    sha256_hex,
    video::{VideoAnalysis, VideoFrame},
};
use office_core::ports::Ports;
use office_testkit::fakes::{FakeClock, FakeLibrary, FakeModels, FakeOrbit, FakeScripts};
use serde_json::{Value, json};
use std::path::Path;

pub(super) const ID: &str = "src-19700101-video";
pub(super) const PATH: &str = "sources/1970/01/src-19700101-video/source.md";
pub(super) fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).unwrap();
    office_testkit::envelope::response(&crate::serve(&request, ports)).unwrap()
}
pub(super) fn fixture(
    settings: Settings,
    vtt: bool,
    private: bool,
) -> (Ports<'static>, FakeModels) {
    let mut library = FakeLibrary::initialized();
    library.settings = settings;
    let markdown = format!(
        "---\nid: {ID}\nkind: video\ntitle: Fixture\nsensitivity: {}\noriginal: {{ file: original.mp4, sha256: {}, bytes: 5, media_type: video/mp4 }}\n{}---\nTranscript.\n",
        if private { "private" } else { "normal" },
        sha256_hex(b"video"),
        if vtt {
            "derived_by: { transcription: vtt-import }\n"
        } else {
            ""
        }
    );
    library
        .insert_video(Path::new(PATH), &markdown, b"video")
        .unwrap();
    let mut models = FakeModels::new();
    models.video_analysis = Some(VideoAnalysis {
        duration_ms: 120_000,
        has_audio: true,
        candidates: vec![
            VideoFrame {
                time_ms: 0,
                score: 400_000,
                dhash: 0,
            },
            VideoFrame {
                time_ms: 5_000,
                score: 900_000,
                dhash: u64::MAX,
            },
            VideoFrame {
                time_ms: 10_000,
                score: 500_000,
                dhash: 1,
            },
            VideoFrame {
                time_ms: 20_000,
                score: 800_000,
                dhash: 0xffff,
            },
            VideoFrame {
                time_ms: 40_000,
                score: 600_000,
                dhash: 0xffff_0000,
            },
            VideoFrame {
                time_ms: 70_000,
                score: 900_000,
                dhash: u64::MAX,
            },
            VideoFrame {
                time_ms: 90_000,
                score: 700_000,
                dhash: 0,
            },
        ],
    });
    models.video_png = b"png-fixture".to_vec();
    let ports = Ports {
        library: Box::new(library),
        models: Box::new(models.clone()),
        scripts: Box::new(FakeScripts::default()),
        orbit: Box::new(FakeOrbit::default()),
        clock: Box::new(FakeClock(std::time::SystemTime::UNIX_EPOCH)),
    };
    (ports, models)
}
pub(super) fn preview(ports: &Ports<'_>) -> Value {
    let response = call(ports, "office.video-plan", json!({"id":ID}));
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}
pub(super) fn confirm(ports: &Ports<'_>) -> Value {
    let preview = preview(ports);
    let response = call(
        ports,
        "office.video-plan",
        json!({"id":ID,"confirm":true,"digest":preview["plan"]["digest"]}),
    );
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}
pub(super) fn private_settings(caps: &[Capability]) -> Settings {
    Settings {
        open_weights: Some(OpenWeights {
            endpoint: "https://private.example/v1".into(),
            model: "fake-private".into(),
            kind: EndpointKind::Company,
            label: "Private".into(),
            capabilities: Some(caps.to_vec()),
            verified_at: Some("1970-01-01T00:00:00Z".into()),
        }),
        ..Settings::default()
    }
}
#[test]
fn selects_spaced_unique_high_scoring_frames_and_counts_without_inventing_rates() {
    let (ports, models) = fixture(Settings::default(), false, false);
    let output = preview(&ports);
    let frames = output["plan"]["frames"].as_array().unwrap();
    assert_eq!(
        frames
            .iter()
            .map(|f| f["time_ms"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [20_000, 40_000, 70_000, 90_000]
    );
    // Four time buckets choose the highest score in each, after spacing/dedup.
    assert!(frames.len() <= 4);
    for pair in frames.windows(2) {
        assert!(
            pair[1]["time_ms"].as_u64().unwrap() - pair[0]["time_ms"].as_u64().unwrap() >= 10_000
        );
        assert!(
            (pair[1]["dhash"].as_u64().unwrap() ^ pair[0]["dhash"].as_u64().unwrap()).count_ones()
                > 5
        );
    }
    assert_eq!(
        output["plan"]["counts"],
        json!({"audio":1,"frames":4,"extract":1})
    );
    assert!(output["plan"]["estimate"].is_null());
    assert_eq!(output["plan"]["rates_unset"].as_array().unwrap().len(), 4);
    assert!(models.sensitivities().is_empty());
}
#[test]
fn vtt_skips_audio_and_prices_from_configured_units() {
    let mut settings = Settings::default();
    for (key, value) in [
        ("costs.currency", json!("USD")),
        ("costs.image", json!(0.01)),
        ("costs.text_1k_tokens", json!(0.02)),
    ] {
        settings = settings.with_setting(key, value).unwrap();
    }
    let (ports, _) = fixture(settings, true, false);
    let output = preview(&ports);
    assert_eq!(output["plan"]["counts"]["audio"], 0);
    assert_eq!(output["plan"]["rates_unset"], json!([]));
    let estimate = &output["plan"]["estimate"];
    assert_eq!(estimate["currency"], "USD");
    assert_eq!(estimate["text_tokens"], 1027);
    assert!((estimate["amount"].as_f64().unwrap() - 0.06054).abs() < 1e-10);
}
#[test]
fn stale_confirmation_writes_nothing_and_matching_digest_confirms_once() {
    let (ports, _) = fixture(Settings::default(), true, false);
    let output = preview(&ports);
    let before = ports.library.read_text(Path::new(PATH)).unwrap();
    let refused = call(
        &ports,
        "office.video-plan",
        json!({"id":ID,"confirm":true,"digest":"stale"}),
    );
    assert_eq!(refused["error"]["code"], "plan_changed");
    assert_eq!(ports.library.read_text(Path::new(PATH)).unwrap(), before);
    let input = json!({"id":ID,"confirm":true,"digest":output["plan"]["digest"]});
    let first = call(&ports, "office.video-plan", input.clone());
    let revision = ports.library.head_commit().unwrap();
    let second = call(&ports, "office.video-plan", input);
    assert_eq!(first, second);
    assert_eq!(ports.library.head_commit().unwrap(), revision);
    ports
        .library
        .set_setting("video.scene_threshold", json!(0.5))
        .unwrap();
    let changed = call(
        &ports,
        "office.video-plan",
        json!({"id":ID,"confirm":true,"digest":output["plan"]["digest"]}),
    );
    assert_eq!(changed["error"]["code"], "plan_changed");
}
#[test]
fn private_preview_names_missing_media_and_never_calls_a_model() {
    for caps in [
        vec![],
        vec![Capability::Text],
        vec![Capability::Text, Capability::Image],
        vec![Capability::Text, Capability::Audio],
    ] {
        let (ports, models) = fixture(private_settings(&caps), false, true);
        let response = call(&ports, "office.video-plan", json!({"id":ID}));
        assert_eq!(
            response["error"]["code"], "private_media_unsupported",
            "{response}"
        );
        let message = response["error"]["message"].as_str().unwrap();
        if !caps.contains(&Capability::Audio) {
            assert!(message.contains("audio"));
        }
        if !caps.contains(&Capability::Image) {
            assert!(message.contains("image"));
        }
        assert!(models.sensitivities().is_empty());
    }
    let (ports, _) = fixture(
        private_settings(&[Capability::Text, Capability::Image]),
        true,
        true,
    );
    assert_eq!(preview(&ports)["plan"]["counts"]["audio"], 0);
}
#[test]
fn marking_private_at_preview_records_the_route_for_every_call_group() {
    let (ports, models) = fixture(private_settings(&Capability::ALL), false, false);
    let response = call(
        &ports,
        "office.video-plan",
        json!({"id":ID,"sensitivity":"private"}),
    );
    assert_eq!(response["ok"], true, "{response}");
    for group in ["audio", "frames", "extract"] {
        assert_eq!(
            response["output"]["plan"]["routes"][group]["route"],
            "private:company"
        );
    }
    assert!(models.sensitivities().is_empty());
}

#[test]
fn private_video_without_a_route_is_refused_and_text_only_failure_is_distinct() {
    let (ports, models) = fixture(Settings::default(), false, true);
    let refused = call(&ports, "office.video-plan", json!({"id":ID}));
    assert_eq!(refused["error"]["code"], "private_media_unsupported");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("audio")
    );
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("image")
    );
    assert!(models.sensitivities().is_empty());
    let (ports, models) = fixture(
        private_settings(&[Capability::Audio, Capability::Image]),
        false,
        true,
    );
    let refused = call(&ports, "office.video-plan", json!({"id":ID}));
    assert_eq!(refused["error"]["code"], "private_endpoint_unverified");
    assert!(models.sensitivities().is_empty());
}
