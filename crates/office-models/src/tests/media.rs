//! Audio splitting: threshold behaviour and the supervised `ffmpeg` call.

use std::io::Write;
use std::path::PathBuf;

use crate::media::{AudioSplitter, needs_split, segment_plan};

#[test]
fn plans_segments_covering_the_recording() {
    assert_eq!(
        segment_plan(1500, 600)
            .into_iter()
            .map(|segment| (segment.start_seconds, segment.length_seconds))
            .collect::<Vec<_>>(),
        vec![(0, 600), (600, 600), (1200, 300)]
    );
    assert!(segment_plan(0, 600).is_empty());
}

#[test]
fn splits_only_above_the_configured_length() {
    assert!(!needs_split(600, 600));
    assert!(needs_split(601, 600));
}

#[test]
fn passes_a_short_recording_through_without_a_subprocess() {
    // The ffmpeg path does not exist: reaching it would fail the call,
    // so success here proves no subprocess ran below the threshold.
    let splitter = AudioSplitter {
        ffmpeg_path: PathBuf::from("/nonexistent/ffmpeg"),
        threshold_seconds: 600,
        max_segment_seconds: 600,
    };
    let outputs = splitter
        .split_if_needed(
            std::path::Path::new("meeting.wav"),
            std::path::Path::new("out"),
            600,
        )
        .expect("short recording passes through");
    assert_eq!(outputs, vec![PathBuf::from("meeting.wav")]);
}

#[test]
fn splits_a_long_recording_with_the_configured_ffmpeg() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ffmpeg = dir.path().join("fake-ffmpeg");
    {
        let mut script = std::fs::File::create(&ffmpeg).expect("write fake ffmpeg");
        // The splitter's last argument is the segment output path.
        script
            .write_all(b"#!/bin/sh\nfor last; do :; done\ntouch \"$last\"\n")
            .expect("write script");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake ffmpeg");
    }
    let input = dir.path().join("meeting.wav");
    std::fs::write(&input, b"fake-audio").expect("write input");
    let out_dir = dir.path().join("segments");
    std::fs::create_dir(&out_dir).expect("output dir");

    let splitter = AudioSplitter {
        ffmpeg_path: ffmpeg,
        threshold_seconds: 600,
        max_segment_seconds: 600,
    };
    let outputs = splitter
        .split_if_needed(&input, &out_dir, 1500)
        .expect("split long recording");
    assert_eq!(outputs.len(), 3);
    for output in &outputs {
        assert!(output.exists(), "segment {} was written", output.display());
    }
}

#[cfg(unix)]
fn executable(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
#[cfg(unix)]
#[test]
fn video_processor_probes_and_hashes_raw_thumbnails_without_an_image_library() {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("ffprobe");
    executable(
        &probe,
        "#!/bin/sh\nprintf '%s' '{\"format\":{\"duration\":\"120.001\"},\"streams\":[{\"codec_type\":\"video\"},{\"codec_type\":\"audio\"}]}'\n",
    );
    let raw = dir.path().join("thumbnails");
    let mut bytes = Vec::new();
    bytes.extend([0u8; 72]);
    for _ in 0..8 {
        bytes.extend([9, 8, 7, 6, 5, 4, 3, 2, 1]);
    }
    std::fs::write(&raw, bytes).unwrap();
    let ffmpeg = dir.path().join("ffmpeg");
    executable(
        &ffmpeg,
        &format!(
            "#!/bin/sh\n[ -f input ] || exit 12\n[ -z \"$META_API_KEY\" ] || exit 13\nprintf 'frame:0 pts:0 pts_time:0\\nlavfi.scene_score=0.4\\nframe:1 pts:10 pts_time:10.5\\nlavfi.scene_score=0.9\\n' > scenes.txt\n/bin/cat '{}'\n",
            raw.display()
        ),
    );
    let processor = crate::media::VideoProcessor {
        ffprobe: probe,
        ffmpeg,
    };
    let analysis = processor
        .analyze(
            b"fixture video",
            &serde_json::Number::from_f64(0.3).unwrap(),
        )
        .unwrap();
    assert_eq!(analysis.duration_ms, 120001);
    assert!(analysis.has_audio);
    assert_eq!(analysis.candidates.len(), 2);
    assert_eq!(analysis.candidates[0].dhash, 0);
    assert_eq!(analysis.candidates[1].dhash, u64::MAX);
    assert_eq!(analysis.candidates[1].time_ms, 10500);
    assert_eq!(analysis.candidates[1].score, 900000);
}

#[cfg(unix)]
#[test]
fn video_scene_decode_uses_the_supported_per_stream_frame_mode() {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("ffprobe");
    executable(
        &probe,
        "#!/bin/sh\nprintf '%s' '{\"format\":{\"duration\":\"60\"},\"streams\":[{\"codec_type\":\"video\"}]}'\n",
    );
    let ffmpeg = dir.path().join("ffmpeg");
    // FFmpeg 9 refuses -vsync before decoding. Enforce its public argv
    // contract while returning an actual thumbnail and matching metadata.
    executable(
        &ffmpeg,
        "#!/bin/sh\nmode=0\nprevious=''\nfor arg; do\n[ \"$arg\" != '-vsync' ] || exit 8\nif [ \"$previous\" = '-fps_mode' ] && [ \"$arg\" = 'vfr' ]; then mode=1; fi\nprevious=\"$arg\"\ndone\n[ \"$mode\" = 1 ] || exit 9\nprintf 'frame:0 pts_time:30\\nlavfi.scene_score=0.5\\n' > scenes.txt\n/bin/dd if=/dev/zero bs=72 count=1 2>/dev/null\n",
    );
    let analysis = crate::media::VideoProcessor {
        ffprobe: probe,
        ffmpeg,
    }
    .analyze(b"synthetic video", &0.into())
    .unwrap();
    assert_eq!(analysis.candidates.len(), 1);
    assert_eq!(analysis.candidates[0].time_ms, 30_000);
}
#[cfg(unix)]
#[test]
fn video_processor_refuses_bad_metadata_and_failed_or_mismatched_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("ffprobe");
    let ffmpeg = dir.path().join("ffmpeg");
    executable(&ffmpeg, "#!/bin/sh\nexit 1\n");
    let processor = crate::media::VideoProcessor {
        ffprobe: probe.clone(),
        ffmpeg: ffmpeg.clone(),
    };
    for duration in ["NaN", "-1", "0", "18446744073709551615.1", "604801"] {
        executable(
            &probe,
            &format!(
                "#!/bin/sh\nprintf '%s' '{{\"format\":{{\"duration\":\"{duration}\"}},\"streams\":[{{\"codec_type\":\"video\"}}]}}'\n"
            ),
        );
        assert_eq!(
            processor.analyze(b"video", &0.into()).unwrap_err().code(),
            "media_processing_failed"
        );
    }
    executable(
        &probe,
        "#!/bin/sh\nprintf '%s' '{\"format\":{\"duration\":\"1\"},\"streams\":[{\"codec_type\":\"video\"}]}'\n",
    );
    assert!(processor.analyze(b"video", &0.into()).is_err());
    executable(
        &ffmpeg,
        "#!/bin/sh\nprintf 'frame:0 pts_time:0\\nlavfi.scene_score=0.5\\n' > scenes.txt\n",
    );
    assert!(processor.analyze(b"video", &0.into()).is_err());
}
#[cfg(unix)]
#[test]
fn frame_extraction_uses_planned_offset_and_640_pixel_png_output() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = dir.path().join("ffmpeg");
    executable(
        &ffmpeg,
        "#!/bin/sh\nseen_scale=0\nseen_time=0\nfor arg; do\n[ \"$arg\" = 'scale=640:-1' ] && seen_scale=1\n[ \"$arg\" = '12.345' ] && seen_time=1\ndone\n[ \"$seen_scale$seen_time\" = 11 ] || exit 7\nprintf '\\211PNG\\015\\012\\032\\012fixture'\n",
    );
    let processor = crate::media::VideoProcessor {
        ffprobe: "/nonexistent".into(),
        ffmpeg,
    };
    assert_eq!(
        processor.frame(b"video", 12345).unwrap(),
        b"\x89PNG\r\n\x1a\nfixture"
    );
}

#[cfg(unix)]
#[test]
fn audio_extraction_decodes_a_bounded_offset_as_wav_and_refuses_invalid_output() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = dir.path().join("ffmpeg");
    executable(
        &ffmpeg,
        "#!/bin/sh\n[ -f input ] || exit 12\n[ -z \"$META_API_KEY\" ] || exit 13\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 14\n[ \"$4\" = '-ss' ] && [ \"$5\" = '600' ] || exit 15\n[ \"$8\" = '-t' ] && [ \"$9\" = '120' ] || exit 16\nprintf 'RIFFxxxxWAVEfixture'\n",
    );
    let processor = crate::media::VideoProcessor {
        ffmpeg: ffmpeg.clone(),
        ffprobe: "/nonexistent".into(),
    };
    assert_eq!(
        processor.audio(b"video", 600, 120).unwrap(),
        b"RIFFxxxxWAVEfixture"
    );
    assert!(processor.audio(b"video", 0, 0).is_err());
    assert!(processor.audio(b"video", 0, 601).is_err());
    executable(&ffmpeg, "#!/bin/sh\nprintf 'invalid'\n");
    assert!(processor.audio(b"video", 0, 1).is_err());
    executable(&ffmpeg, "#!/bin/sh\nexit 1\n");
    assert!(processor.audio(b"video", 0, 1).is_err());
}
