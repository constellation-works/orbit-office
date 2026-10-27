use super::video_plan::{ID, call, fixture};
use office_core::domain::settings::Settings;
use serde_json::json;

#[test]
fn imported_vtt_is_a_noop_without_inference_or_confirmation() {
    let (ports, models) = fixture(Settings::default(), true, false);
    let before = ports.library.head_commit().unwrap();
    let response = call(&ports, "office.transcribe", json!({"id":ID}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["noop"], true);
    assert!(models.sensitivities().is_empty());
    assert_eq!(ports.library.head_commit().unwrap(), before);
}

use super::video_plan::{PATH, confirm};
use office_core::domain::source::{sha256_hex, video};
use office_core::ports::{Transcript, TranscriptSegment};
use std::path::Path;

#[test]
fn text_only_chunks_get_offset_plus_position_and_budget_stops_a_repeat() {
    let (mut ports, mut models) = fixture(Settings::default(), false, false);
    models.video_analysis.as_mut().unwrap().duration_ms = 1_201_000;
    models.transcript = Transcript {
        text: "Ada: First\nBen: Second".into(),
        ..Transcript::default()
    };
    ports.models = Box::new(models.clone());
    let plan = confirm(&ports);
    assert_eq!(plan["plan"]["counts"]["audio"], 3);
    let response = call(&ports, "office.transcribe", json!({"id":ID}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["chunks"], 3);
    assert_eq!(
        *models.audio_windows.borrow(),
        [(0, 600), (600, 600), (1200, 1)]
    );
    let body = ports.library.read_source(ID).unwrap().body;
    for stamp in ["00:00:00", "00:00:01", "00:10:00", "00:10:01", "00:20:00"] {
        assert!(body.contains(&format!("#t={stamp}")), "{body}");
    }
    let text = ports.library.read_text(Path::new(PATH)).unwrap();
    assert!(text.contains("transcription:") && text.contains("fake-hosted"));
    assert_eq!(video::state(&text).unwrap().unwrap().calls.audio, 3);
    assert_eq!(
        call(&ports, "office.transcribe", json!({"id":ID}))["error"]["code"],
        "plan_exceeded"
    );
    assert_eq!(models.sensitivities().len(), 3);
    // Generated transcript hashes keep the original approved plan usable by frames.
    assert_eq!(call(&ports, "office.frames", json!({"id":ID}))["ok"], true);
}

#[test]
fn returned_timestamps_shift_onto_the_recording_and_corrections_keep_old_versions() {
    let (mut ports, mut models) = fixture(Settings::default(), false, false);
    models.video_analysis.as_mut().unwrap().duration_ms = 601_000;
    models.transcript = Transcript {
        text: "Speech".into(),
        segments: vec![TranscriptSegment {
            speaker: Some("Ada".into()),
            start_ms: 250,
            end_ms: 500,
            text: "Speech".into(),
        }],
        chunks: vec![],
    };
    ports.models = Box::new(models.clone());
    confirm(&ports);
    assert_eq!(
        call(&ports, "office.transcribe", json!({"id":ID}))["ok"],
        true
    );
    let first = ports
        .library
        .read_text(&Path::new(PATH).with_file_name("v1.md"))
        .unwrap();
    assert!(
        ports
            .library
            .read_source(ID)
            .unwrap()
            .body
            .contains("[00:10:00] Ada:")
    );
    // Preview/confirm the new transcript authorizes a correction, never the old cap.
    confirm(&ports);
    let response = call(&ports, "office.transcribe", json!({"id":ID}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        ports
            .library
            .read_text(&Path::new(PATH).with_file_name("v1.md"))
            .unwrap(),
        first
    );
    assert!(
        ports
            .library
            .read_text(&Path::new(PATH).with_file_name("v3.md"))
            .unwrap()
            .contains("supersedes:")
    );
}

#[test]
fn a_reserved_uncertain_chunk_is_refused_before_inference() {
    let (ports, models) = fixture(Settings::default(), false, false);
    let confirmed = confirm(&ports);
    let source = ports.library.read_video(ID).unwrap();
    ports
        .library
        .mutate_video(
            ID,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::ReserveProcessing {
                digest: confirmed["plan"]["digest"].as_str().unwrap().into(),
                group: "audio".into(),
                index: 0,
            },
        )
        .unwrap();
    let result = call(&ports, "office.transcribe", json!({"id":ID}));
    assert_eq!(result["error"]["code"], "plan_exceeded");
    assert!(models.sensitivities().is_empty());
    assert!(models.audio_windows.borrow().is_empty());
}

#[test]
fn private_transcription_preserves_partial_results_and_failed_final_audio_cannot_be_extracted() {
    use super::video_plan::private_settings;
    use office_core::domain::endpoint::Capability;
    let (mut ports, mut models) = fixture(private_settings(&Capability::ALL), false, true);
    models.video_analysis.as_mut().unwrap().duration_ms = 601_000;
    models.transcribe_failure = Some(1);
    ports.models = Box::new(models.clone());
    confirm(&ports);
    let result = call(&ports, "office.transcribe", json!({"id":ID}));
    assert_eq!(result["error"]["code"], "model_unavailable");
    let source = ports.library.read_video(ID).unwrap();
    let state = video::state(&source.markdown).unwrap().unwrap();
    assert_eq!(state.calls.audio, 2);
    assert_eq!(state.audio_completed, 1);
    assert!(
        ports
            .library
            .read_source(ID)
            .unwrap()
            .body
            .contains("fake transcript")
    );
    assert_eq!(models.sensitivities().len(), 2);
    assert!(
        models
            .sensitivities()
            .iter()
            .all(|s| s.as_str() == "private")
    );
    assert_eq!(
        call(&ports, "office.transcribe", json!({"id":ID}))["error"]["code"],
        "plan_exceeded"
    );
    assert_eq!(call(&ports, "office.frames", json!({"id":ID}))["ok"], true);
    let refused = call(
        &ports,
        "office.extract",
        json!({"id":ID,"profile":"kt","task":"task-fixture"}),
    );
    assert_eq!(refused["error"]["code"], "processing_incomplete");
    assert!(models.extract_requests.borrow().is_empty());
}
