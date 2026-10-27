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
