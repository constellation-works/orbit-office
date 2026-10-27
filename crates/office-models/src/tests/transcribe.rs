//! Transcription through the Models port, including long recordings.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use base64::Engine;
use office_common::Sensitivity;
use office_core::ports::{Models, TranscribeRequest};
use serde_json::Value;

use super::{FakeServer, chat_reply, client, hosted_config};

struct Ffmpeg {
    directory: tempfile::TempDir,
    executable: PathBuf,
}

impl Ffmpeg {
    fn new(body: &str) -> Self {
        let directory = tempfile::tempdir().expect("ffmpeg fixture");
        let executable = directory.path().join("ffmpeg");
        let script = format!(
            "#!/bin/sh\nset -eu\nroot=${{0%/*}}\nprintf '%s\\n' \"$@\" >> \"$root/calls\"\ncp \"$7\" \"$root/input\"\nprintf '%s' \"$TMPDIR\" > \"$root/staging\"\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 81\n{body}\n"
        );
        std::fs::write(&executable, script).expect("fake ffmpeg script");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("chmod fake ffmpeg");
        Self {
            directory,
            executable,
        }
    }

    fn splitting() -> Self {
        // Distinct chunk bytes prove that the client sends ffmpeg's outputs,
        // rather than repeatedly sending the original recording.
        Self::new("for output; do :; done\nprintf 'audio-%s-%s' \"$3\" \"$5\" > \"$output\"")
    }

    fn assert_not_run(&self) {
        assert!(!self.directory.path().join("calls").exists());
    }

    fn assert_cleaned(&self) {
        let staging = std::fs::read_to_string(self.directory.path().join("staging"))
            .expect("staging path captured by ffmpeg");
        assert!(
            !std::path::Path::new(&staging).exists(),
            "staged audio leaked"
        );
    }
}

fn request(duration_seconds: u64, sensitivity: Sensitivity) -> TranscribeRequest {
    TranscribeRequest {
        audio: recording(duration_seconds),
        duration_seconds,
        mime: "audio/wav".into(),
        sensitivity,
        language: Some("English".into()),
    }
}

fn assert_chunks(server: &FakeServer, chunks: &[&[u8]], token: &str, model: &str) {
    let requests = server.requests();
    assert_eq!(requests.len(), chunks.len());
    for (request, chunk) in requests.iter().zip(chunks) {
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.authorization.as_deref(), Some(token));
        let body: Value = serde_json::from_str(&request.body).expect("JSON request");
        assert_eq!(body["model"], model);
        let content = &body["messages"][0]["content"];
        let instruction = content[0]["text"].as_str().expect("instruction");
        assert!(
            instruction
                .starts_with("Transcribe this audio verbatim. The spoken language is English."),
            "language hint must stay on the transcription request"
        );
        assert!(
            instruction.contains("segments")
                && instruction.contains("start_ms")
                && instruction.contains("speaker"),
            "transcription must ask for speaker turns and timestamps"
        );
        assert_eq!(body["response_format"]["json_schema"]["name"], "transcript");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(content[1]["input_audio"]["format"], "wav");
        let audio = base64::engine::general_purpose::STANDARD
            .decode(
                content[1]["input_audio"]["data"]
                    .as_str()
                    .expect("base64 audio"),
            )
            .expect("decode audio");
        assert_eq!(&audio, chunk);
    }
}

// A valid PCM WAV at one sample per second keeps even long fixtures tiny.
fn recording(duration_seconds: u64) -> Vec<u8> {
    let data_len = u32::try_from(duration_seconds * 2).expect("small fixture");
    let samples = vec![0_u8; usize::try_from(data_len).expect("small fixture")];
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(data_len + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&1_u32.to_le_bytes()); // sample rate
    wav.extend_from_slice(&2_u32.to_le_bytes()); // bytes per second
    wav.extend_from_slice(&2_u16.to_le_bytes()); // block alignment
    wav.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

#[test]
fn transcribes_long_recordings_in_multiple_routed_requests() {
    let ffmpeg = Ffmpeg::splitting();
    let server = FakeServer::start_replies(vec![
        (200, chat_reply("first")),
        (200, chat_reply("second")),
        (200, chat_reply("third")),
    ]);
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let transcript = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect("transcribe long recording");
    assert_eq!(transcript.text, "first\nsecond\nthird");
    assert!(transcript.segments.is_empty());
    assert_chunks(
        &server,
        &[b"audio-0-600", b"audio-600-600", b"audio-1200-300"],
        "Bearer test-meta-key",
        "muse-spark-1.3",
    );
    assert_eq!(
        std::fs::read(ffmpeg.directory.path().join("input")).expect("staged original"),
        recording(1500)
    );
    let calls =
        std::fs::read_to_string(ffmpeg.directory.path().join("calls")).expect("ffmpeg args");
    let args: Vec<_> = calls.lines().collect();
    assert_eq!(args.len(), 30);
    for (args, (start, length)) in
        args.chunks_exact(10)
            .zip([("0", "600"), ("600", "600"), ("1200", "300")])
    {
        assert_eq!(&args[..6], &["-y", "-ss", start, "-t", length, "-i"]);
        assert_eq!(&args[7..9], &["-c", "copy"]);
        assert_eq!(
            std::path::Path::new(args[9])
                .extension()
                .expect("WAV suffix"),
            "wav"
        );
    }
    ffmpeg.assert_cleaned();
}

#[test]
fn offsets_reported_turns_by_each_chunks_start() {
    let ffmpeg = Ffmpeg::splitting();
    let server = FakeServer::start_replies(vec![
        (200, turn_reply("first", "Jane", 0, 1_500)),
        (200, turn_reply("second", "Daniel", 1_000, 2_500)),
        (200, turn_reply("third", "Jane", 500, 900)),
    ]);
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let transcript = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect("transcribe long recording");
    assert_eq!(transcript.text, "first\nsecond\nthird");
    assert_eq!(transcript.segments.len(), 3);
    assert_eq!(transcript.segments[0].start_ms, 0);
    assert_eq!(transcript.segments[0].end_ms, 1_500);
    assert_eq!(transcript.segments[1].speaker.as_deref(), Some("Daniel"));
    assert_eq!(transcript.segments[1].start_ms, 601_000);
    assert_eq!(transcript.segments[1].end_ms, 602_500);
    assert_eq!(transcript.segments[2].start_ms, 1_200_500);
    assert_eq!(transcript.segments[2].end_ms, 1_200_900);
    assert_eq!(
        office_core::domain::source::meeting_body(&transcript),
        "[00:00:00] Jane: first\n\n[00:10:01] Daniel: second\n\n[00:20:00] Jane: third"
    );
    ffmpeg.assert_cleaned();
}

fn turn_reply(text: &str, speaker: &str, start_ms: u64, end_ms: u64) -> String {
    chat_reply(&format!(
        r#"{{"text":"{text}","segments":[{{"speaker":"{speaker}","start_ms":{start_ms},"end_ms":{end_ms},"text":"{text}"}}]}}"#
    ))
}

#[test]
fn routes_every_private_chunk_to_open_weights_with_configured_lengths() {
    let ffmpeg = Ffmpeg::splitting();
    let hosted = FakeServer::start(200, chat_reply("must not receive private audio"));
    let open = FakeServer::start(200, chat_reply("private"));
    let mut config = hosted_config(&hosted.base_url);
    config.open_weights_endpoint = Some(open.base_url.clone());
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    config.audio_splitter.threshold_seconds = 30;
    config.audio_splitter.max_segment_seconds = 20;
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let transcript = models
        .transcribe(&request(45, Sensitivity::Private))
        .expect("private audio");
    assert_eq!(transcript.text, "private\nprivate\nprivate");
    assert!(hosted.requests().is_empty());
    assert_chunks(
        &open,
        &[b"audio-0-20", b"audio-20-20", b"audio-40-5"],
        "Bearer test-baseten-token",
        "meta-models/muse-spark-open-1026",
    );
    ffmpeg.assert_cleaned();
}

#[test]
fn at_or_below_threshold_never_runs_ffmpeg() {
    let ffmpeg = Ffmpeg::new("exit 99");
    let server = FakeServer::start(200, chat_reply("unsplit"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    config.audio_splitter.threshold_seconds = 30;
    config.audio_splitter.max_segment_seconds = 20;
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    for duration in [0, 29, 30] {
        assert_eq!(
            models
                .transcribe(&request(duration, Sensitivity::Normal))
                .expect("unsplit")
                .text,
            "unsplit"
        );
    }
    let audio = [0, 29, 30].map(recording);
    assert_chunks(
        &server,
        &[&audio[0], &audio[1], &audio[2]],
        "Bearer test-meta-key",
        "muse-spark-1.3",
    );
    ffmpeg.assert_not_run();
}

#[test]
fn split_failure_returns_an_error_without_sending_audio() {
    let ffmpeg = Ffmpeg::new("exit 99");
    let server = FakeServer::start(200, chat_reply("must not receive audio"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let error = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect_err("split failed");
    assert_eq!(error.code(), "media_split_failed");
    assert!(!error.retryable());
    assert!(server.requests().is_empty());
    ffmpeg.assert_cleaned();
}

#[test]
fn later_chunk_failure_returns_an_error_and_cleans_staging() {
    let ffmpeg = Ffmpeg::splitting();
    let server = FakeServer::start_replies(vec![(200, chat_reply("first")), (503, "{}".into())]);
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let error = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect_err("no partial success");
    assert_eq!(error.code(), "model_unavailable");
    assert!(error.retryable());
    assert_eq!(server.requests().len(), 2);
    ffmpeg.assert_cleaned();
}

#[test]
fn refuses_private_audio_without_an_endpoint_before_splitting() {
    let ffmpeg = Ffmpeg::new("exit 99");
    let server = FakeServer::start(200, chat_reply("must not receive private audio"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let error = models
        .transcribe(&request(1500, Sensitivity::Private))
        .expect_err("private refused");
    assert_eq!(error.code(), "private_endpoint_unconfigured");
    assert!(server.requests().is_empty());
    ffmpeg.assert_not_run();
}

#[test]
fn refuses_zero_chunk_length_instead_of_returning_an_empty_transcript() {
    let ffmpeg = Ffmpeg::new("exit 99");
    let server = FakeServer::start(200, chat_reply("must not receive audio"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    config.audio_splitter.max_segment_seconds = 0;
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let error = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect_err("zero chunk length");
    assert_eq!(error.code(), "media_split_failed");
    assert!(server.requests().is_empty());
    ffmpeg.assert_not_run();
}

#[test]
fn refuses_unsupported_long_audio_before_starting_ffmpeg() {
    let ffmpeg = Ffmpeg::new("exit 99");
    let server = FakeServer::start(200, chat_reply("must not receive audio"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let mut request = request(1500, Sensitivity::Normal);
    request.mime = "audio/../../unsupported".into();
    let error = models
        .transcribe(&request)
        .expect_err("unsupported container");
    assert_eq!(error.code(), "media_unsupported_format");
    assert!(server.requests().is_empty());
    ffmpeg.assert_not_run();
}

#[test]
fn missing_ffmpeg_output_fails_without_sending_audio_and_cleans_staging() {
    let ffmpeg = Ffmpeg::new("exit 0");
    let server = FakeServer::start(200, chat_reply("must not receive audio"));
    let mut config = hosted_config(&server.base_url);
    config.audio_splitter.ffmpeg_path = ffmpeg.executable.clone();
    let adapter = client(config);
    let models: &dyn Models = &adapter;
    let error = models
        .transcribe(&request(1500, Sensitivity::Normal))
        .expect_err("missing split output");
    assert_eq!(error.code(), "media_io_failed");
    assert!(server.requests().is_empty());
    ffmpeg.assert_cleaned();
}
