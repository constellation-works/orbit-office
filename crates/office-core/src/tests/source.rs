//! Source metadata, sensitivity, anchors and VTT import (specs/library.md).

#[test]
fn source_metadata_retains_quoted_original_numbers_and_versions() {
    let meta = crate::domain::source::parse_source_meta(
        "---\nid: src-fixture\nversion: \"3\"\noriginal: { bytes: \"5\", file: \"original.mp4\", media_type: \"video/mp4\", sha256: \"abc\" }\n---\nBody.",
    );
    assert_eq!(meta.version, 3);
    assert_eq!(meta.bytes, Some(5));
    assert_eq!(meta.original_file.as_deref(), Some("original.mp4"));
    assert_eq!(meta.sha256.as_deref(), Some("abc"));
    assert_eq!(meta.media_type.as_deref(), Some("video/mp4"));
}

#[test]
fn sensitivity_updates_preserve_unrelated_metadata_and_refuse_invalid_privacy_state() {
    use crate::domain::source::access::with_sensitivity;
    let original = "---\nid: src-fixture\n# Comment\nsensitivity : normal # annotation\nversion: 3\nderived_by: { transcription: vtt-import }\n---\nBody\nsensitivity: normal\n";
    let updated = with_sensitivity(original, "private").unwrap();
    assert_eq!(
        updated,
        original.replacen(
            "sensitivity : normal # annotation",
            "sensitivity: private",
            1
        )
    );
    assert_eq!(with_sensitivity(&updated, "private").unwrap(), updated);
    assert_eq!(
        with_sensitivity(&updated, "normal").unwrap_err().code(),
        "sensitivity_downgrade_refused"
    );
    let missing = "---\nid: src-fixture\n---\nBody.";
    assert_eq!(
        with_sensitivity(missing, "private").unwrap(),
        "---\nsensitivity: private\nid: src-fixture\n---\nBody."
    );
    assert_eq!(
        with_sensitivity(
            "---\nsensitivity: private\nsensitivity: normal\n---\n",
            "normal"
        )
        .unwrap_err()
        .code(),
        "invalid_frontmatter"
    );
    assert_eq!(
        with_sensitivity("---\nsensitivity: unknown\n---\n", "normal")
            .unwrap_err()
            .code(),
        "invalid_source"
    );
}

#[test]
fn source_participants_preserve_quoted_punctuation_and_block_lists() {
    use crate::domain::source::parse_source_participants;

    for metadata in [
        "participants: [\"Nair, Priya\", 'O''Brien', \"C:\\\\Team\"]",
        "participants:\n  - \"Nair, Priya\"\n  - 'O''Brien'\n  - \"C:\\\\Team\"",
    ] {
        assert_eq!(
            parse_source_participants(&format!("---\nid: src-people\n{metadata}\n---\nBody"))
                .expect("participants"),
            ["Nair, Priya", "O'Brien", "C:\\Team"]
        );
    }
    assert!(
        parse_source_participants("---\nid: src-empty\n---\nBody")
            .expect("optional participants")
            .is_empty()
    );
    let error = parse_source_participants("---\nparticipants: [true]\n---\nBody")
        .expect_err("non-string participant refused");
    assert_eq!(error.code(), "invalid_frontmatter");
}

use crate::domain::source::{
    RenderSource, SourceKind, anchor_span, parse_anchor, parse_source_meta, render_source,
    source_body, vtt_to_body,
};
use office_common::Sensitivity;

#[test]
fn anchors_distinguish_malformed_from_out_of_range() {
    let body = "one\ntwo\nthree";
    assert_eq!(
        anchor_span(body, parse_anchor("#L2-3").expect("anchor"), "#L2-3").expect("span"),
        "two\nthree"
    );
    assert_eq!(
        parse_anchor("banana").expect_err("malformed").code(),
        "anchor_invalid"
    );
    assert_eq!(
        anchor_span(body, parse_anchor("#L9").expect("anchor"), "#L9")
            .expect_err("range")
            .code(),
        "anchor_out_of_range"
    );
}

#[test]
fn timestamp_overflow_is_refused_by_anchor_parsing() {
    for raw in [
        "#t=1152921504606846976:00:00",
        "#t=5124095576030431:00:16",
        "#t=18446744073709551615:59:59",
    ] {
        assert_eq!(parse_anchor(raw).expect_err(raw).code(), "anchor_invalid");
    }
}

#[test]
fn timestamp_overflow_in_source_turns_never_resolves_as_zero() {
    let body = "[1152921504606846976:00:00] Jane: overflow";
    assert_eq!(
        anchor_span(body, parse_anchor("t=00:00:00").unwrap(), "t=00:00:00")
            .expect_err("overflowing turn is not a valid location")
            .code(),
        "anchor_out_of_range"
    );
}

#[test]
fn timestamp_boundaries_and_hours_over_99_resolve_without_milliseconds() {
    for (stamp, seconds) in [
        ("100:00:00", 360_000),
        ("5124095576030:25:51", u64::MAX / 1_000),
        ("5124095576040:25:51", 18_446_744_073_745_551),
        ("5124095576030431:00:15", u64::MAX),
    ] {
        let raw = format!("t={stamp}");
        let parsed = parse_anchor(&raw).expect("representable seconds");
        assert_eq!(parsed, crate::domain::source::Anchor::Time { seconds });
        let body = format!("[{stamp}] Jane: valid");
        assert_eq!(anchor_span(&body, parsed, &raw).unwrap(), body);
    }
}

#[test]
fn time_anchors_resolve_to_the_turn_in_progress() {
    let body = "[00:10:00] Jane: one\n\n[00:12:31] Dee: two";
    let span = |raw: &str| anchor_span(body, parse_anchor(raw).expect("anchor"), raw);
    assert_eq!(
        span("#t=00:10:00").expect("first turn"),
        "[00:10:00] Jane: one"
    );
    assert_eq!(
        span("#t=00:11:00").expect("interior"),
        "[00:10:00] Jane: one"
    );
    assert_eq!(
        span("#t=00:12:31").expect("last turn"),
        "[00:12:31] Dee: two"
    );
    for raw in ["#t=00:09:59", "#t=00:12:32"] {
        assert_eq!(
            span(raw).expect_err("out of range").code(),
            "anchor_out_of_range",
            "{raw}"
        );
    }
    assert_eq!(
        anchor_span(
            "no turns",
            parse_anchor("#t=00:00:00").expect("anchor"),
            "#t=00:00:00"
        )
        .expect_err("no turns")
        .code(),
        "anchor_out_of_range"
    );
    assert_eq!(
        parse_anchor("#t=99:99:99").expect_err("malformed").code(),
        "anchor_invalid"
    );
}

#[test]
fn time_anchors_resolve_single_newline_turns() {
    let body = "[00:10:00] Jane: one\n[00:12:31] Dee: two";
    let span = |raw: &str| anchor_span(body, parse_anchor(raw).expect("anchor"), raw);
    assert_eq!(
        span("#t=00:10:00").expect("first turn"),
        "[00:10:00] Jane: one"
    );
    assert_eq!(
        span("#t=00:11:00").expect("interior"),
        "[00:10:00] Jane: one"
    );
    assert_eq!(
        span("#t=00:12:31").expect("last turn"),
        "[00:12:31] Dee: two"
    );
    for raw in ["#t=00:09:59", "#t=00:12:32"] {
        assert_eq!(
            span(raw).expect_err("out of range").code(),
            "anchor_out_of_range",
            "{raw}"
        );
    }
}

#[test]
fn time_anchors_keep_continuation_lines_with_their_turn() {
    let body = "[00:10:00] Jane: one\nstill Jane\n[00:12:31] Dee: two";
    let span = |raw: &str| anchor_span(body, parse_anchor(raw).expect("anchor"), raw);
    assert_eq!(
        span("#t=00:10:30").expect("turn with continuation"),
        "[00:10:00] Jane: one\nstill Jane"
    );
    assert_eq!(
        span("#t=00:12:31").expect("last turn"),
        "[00:12:31] Dee: two"
    );
}

#[test]
fn vtt_import_keeps_speakers_and_cue_times() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Jane>Hi there.</v>\n\n00:01:05.500 --> 00:01:07.000\nDaniel: Next point.\n";
    assert_eq!(
        vtt_to_body(vtt).expect("valid VTT"),
        "[00:00:01] Jane: Hi there.\n\n[00:01:05] Daniel: Next point."
    );
}

#[test]
fn vtt_import_refuses_timestamp_overflow_at_both_endpoints() {
    for stamp in [
        "1152921504606846976:00:00.000",
        "5124095576030431:00:16.000",
        "5124095576030:25:52.000",
        "5124095576030:25:51.616",
        "18446744073709551615:00.000",
    ] {
        for (start, end) in [(stamp, "00:00:03.000"), ("00:00:01.000", stamp)] {
            let vtt = format!("WEBVTT\n\n{start} --> {end}\nJane: overflow\n");
            assert_eq!(
                vtt_to_body(&vtt).expect_err(stamp).code(),
                "invalid_argument"
            );
        }
    }
}

#[test]
fn vtt_import_preserves_supported_timestamp_boundaries_and_cue_settings() {
    for (start, end, stamp) in [
        ("100:00:01.000", "100:00:02.000", "100:00:01"),
        ("01:05.500", "01:07.000", "00:01:05"),
        (
            "5124095576030:25:51.614",
            "5124095576030:25:51.615",
            "5124095576030:25:51",
        ),
        (
            "5124095576030:25:51.615",
            "5124095576030:25:51.615",
            "5124095576030:25:51",
        ),
    ] {
        let vtt = format!("WEBVTT\n\n{start} --> {end} align:start\nJane: valid\n");
        let body = vtt_to_body(&vtt).expect("representable milliseconds");
        assert_eq!(body, format!("[{stamp}] Jane: valid"));
        let raw = format!("t={stamp}");
        assert_eq!(
            anchor_span(&body, parse_anchor(&raw).unwrap(), &raw).unwrap(),
            body
        );
    }
}

#[test]
fn vtt_import_refuses_malformed_cue_timestamps() {
    for timing in [
        "not-a-time --> 00:00:03.000",
        "00:00:01.000 --> missing",
        "00:60:00.000 --> 00:61:00.000",
        "00:00:60.000 --> 00:01:02.000",
    ] {
        let vtt = format!("WEBVTT\n\n{timing}\nJane: invalid\n");
        assert_eq!(
            vtt_to_body(&vtt).expect_err(timing).code(),
            "invalid_argument"
        );
    }
}

#[test]
fn vtt_import_keeps_numeric_and_keyword_prefixed_cue_text() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n1234\n\n00:00:03.000 --> 00:00:04.000\nNOTE: the answer\n\n00:00:05.000 --> 00:00:06.000\nDaniel: It is\n2026\n\n00:00:07.000 --> 00:00:08.000\nWEBVTT is the format\n";
    assert_eq!(
        vtt_to_body(vtt).expect("valid VTT"),
        "[00:00:01] Speaker: 1234\n\n[00:00:03] NOTE: the answer\n\n[00:00:05] Daniel: It is 2026\n\n[00:00:07] Speaker: WEBVTT is the format"
    );
}

#[test]
fn vtt_import_skips_header_cue_identifiers_and_comment_blocks() {
    let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE producer comment\nthat spans lines\n\nSTYLE\n::cue { color: red }\n\n7\n00:00:01.000 --> 00:00:02.000\n<v Jane>Hi there.</v>\n\nNOTE editor aside\n\n8\n00:00:03.000 --> 00:00:04.000\nDaniel: Next point.\n";
    assert_eq!(
        vtt_to_body(vtt).expect("valid VTT"),
        "[00:00:01] Jane: Hi there.\n\n[00:00:03] Daniel: Next point."
    );
}

#[test]
fn rendered_source_frontmatter_round_trips() {
    let text = render_source(&RenderSource {
        id: "src-19700101-hello",
        kind: SourceKind::Video,
        title: "Hello",
        captured_at: "1970-01-01T00:00:00+00:00",
        captured_by: "upload",
        origin: None,
        occurred_at: None,
        participants: &[],
        event: None,
        tags: &[],
        sensitivity: Sensitivity::Normal,
        original: Some(("original.mp4", "abc123", 42, "video/mp4")),
        derived_transcription: Some("vtt-import"),
        retention_days: Some(90),
        body: "body",
    });
    let meta = parse_source_meta(&text);
    assert_eq!(meta.id, "src-19700101-hello");
    assert_eq!(meta.kind, "video");
    assert_eq!(meta.sha256.as_deref(), Some("abc123"));
    assert!(text.contains("derived_by: { transcription: vtt-import }"));
    assert!(text.contains("original_until: 1970-04-01"));
}

#[test]
fn rendered_source_title_with_quotes_and_backslashes_round_trips() {
    let titles = [
        r#"Say "Hello" from C:\docs"#,
        r#"Literal \"escape\" and \\ pair"#,
        r"C:\docs\file.txt",
    ];
    for title in titles {
        let text = render_source(&RenderSource {
            id: "src-19700101-hello",
            kind: SourceKind::Note,
            title,
            captured_at: "1970-01-01T00:00:00+00:00",
            captured_by: "upload",
            origin: None,
            occurred_at: None,
            participants: &[],
            event: None,
            tags: &[],
            sensitivity: Sensitivity::Normal,
            original: Some((r"C:\docs\file.txt", "abc123", 42, "text/plain")),
            derived_transcription: None,
            retention_days: None,
            body: "body",
        });
        let meta = parse_source_meta(&text);
        assert_eq!(meta.title, title, "rendered: {text}");
        // `original.file` is written unquoted, so its backslashes are literal
        // and must not be decoded.
        assert_eq!(meta.original_file.as_deref(), Some(r"C:\docs\file.txt"));
    }
    // Unsupported escapes in a hand-written quoted scalar stay literal.
    let meta = parse_source_meta("---\ntitle: \"a\\nb\"\n---\nbody");
    assert_eq!(meta.title, r"a\nb");
}

#[test]
fn rendered_source_keeps_untrusted_metadata_inside_frontmatter() {
    use crate::domain::knowledge::DocumentRecord;
    use std::path::Path;

    let payloads = [
        "paste\nsensitivity: normal\n---\ninjected",
        "Alice]\nsensitivity: normal\n---\ninjected",
        "quote \" slash \\ tab\t return\r nul\0",
    ];
    for payload in payloads {
        let participants = vec![payload.to_string()];
        let text = render_source(&RenderSource {
            id: "src-19700101-private",
            kind: SourceKind::Note,
            title: "Private",
            captured_at: "1970-01-01T00:00:00+00:00",
            captured_by: payload,
            origin: None,
            occurred_at: Some(payload),
            participants: &participants,
            event: None,
            tags: &[],
            sensitivity: Sensitivity::Private,
            original: None,
            derived_transcription: None,
            retention_days: None,
            body: "private body\n",
        });
        assert_eq!(parse_source_meta(&text).sensitivity, "private");
        let record = DocumentRecord::from_markdown(Path::new("sources/source.md"), &text);
        assert_eq!(record.sensitivity, "private");
        assert_eq!(source_body(&text), "private body\n");
        for key in ["captured_by", "occurred_at"] {
            let line = text
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key}: ")))
                .unwrap();
            assert_eq!(serde_json::from_str::<String>(line).unwrap(), payload);
        }
        let line = text
            .lines()
            .find_map(|line| line.strip_prefix("participants: "))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(line).unwrap(),
            participants
        );
    }
}

#[test]
fn processing_markdown_bytes_are_exact_for_nested_empty_escaped_unicode_null_and_bool_values() {
    use crate::domain::endpoint::ModelRoute;
    use crate::domain::source::processing_markdown;
    use crate::ports::DerivedBy;
    let markdown = "---\nid: src-flow\ntitle: Café\nempty_map: {}\nempty_list: []\nnested: { flag: true, inner: [\"a\\\"b\", \"🌍\"], missing: null }\n---\nBody.";
    let derived = DerivedBy {
        route: ModelRoute::Hosted,
        model: "m\"🌍".into(),
    };
    let (next, snapshot) = processing_markdown(markdown, None, &derived).unwrap();
    assert_eq!(snapshot, None);
    assert_eq!(
        next,
        "---\n\
         id: \"src-flow\"\n\
         title: \"Café\"\n\
         empty_map: {  }\n\
         empty_list: []\n\
         nested: { flag: true, inner: [\"a\\\"b\", \"🌍\"], missing: null }\n\
         derived_by: { extraction: { model: \"m\\\"🌍\", route: \"hosted\" } }\n\
         ---\nBody."
    );
}

#[test]
fn video_ledger_bytes_keep_quoted_plan_and_numeric_counts() {
    use crate::domain::settings::Settings;
    use crate::domain::source::video;
    let source = video::VideoSource {
        path: "sources/1970/01/src-video/source.md".into(),
        markdown: "---\nid: src-video\nkind: video\nsensitivity: normal\n---\n".into(),
        bytes: b"video".to_vec(),
    };
    let settings = Settings::default();
    let plan = video::plan(
        &source,
        &video::VideoAnalysis {
            duration_ms: 60_000,
            has_audio: true,
            candidates: Vec::new(),
        },
        &settings,
        "normal",
    )
    .unwrap();
    let digest = plan.digest.clone();
    let preview = video::apply(
        &source,
        &settings,
        video::VideoMutation::Plan {
            plan: Box::new(plan),
            confirm_digest: None,
            at: "1970-01-01T00:00:00Z".into(),
        },
    )
    .unwrap();
    assert!(
        preview.markdown.contains(
            "video: { audio_completed: 0, calls: { audio: 0, extract: 0, frames: 0 }, confirmed_at: null, duration_s: 60, frames_read: 0, plan: \""
        ),
        "{}",
        preview.markdown
    );
    assert!(
        preview
            .markdown
            .contains(&format!("plan_digest: \"{digest}\"")),
        "{}",
        preview.markdown
    );
    assert!(
        preview.markdown.contains(&format!(
            ", plan_digest: \"{digest}\", readings: \"[]\", transcription_sha256: null }}"
        )),
        "{}",
        preview.markdown
    );
    let state = video::state(&preview.markdown).unwrap().unwrap();
    assert_eq!(state.plan.digest, digest);
}

#[test]
fn video_selection_keeps_highest_scores_spread_across_the_recording() {
    use crate::domain::source::video::{VideoAnalysis, VideoFrame, select_frames};
    let analysis = VideoAnalysis {
        duration_ms: 3600000,
        has_audio: true,
        candidates: vec![
            VideoFrame {
                time_ms: 10000,
                score: 900000,
                dhash: 0,
            },
            VideoFrame {
                time_ms: 20000,
                score: 950000,
                dhash: u64::MAX,
            },
            VideoFrame {
                time_ms: 1900000,
                score: 600000,
                dhash: 0,
            },
            VideoFrame {
                time_ms: 3599999,
                score: 700000,
                dhash: 0xffff,
            },
        ],
    };
    let selected = select_frames(&analysis, 2);
    assert_eq!(
        selected.iter().map(|f| f.time_ms).collect::<Vec<_>>(),
        [20000, 3599999]
    );
    let mut duplicate = analysis;
    duplicate.candidates[1].dhash = 31; // exactly five differing bits: dropped
    let selected = select_frames(&duplicate, 600);
    assert!(!selected.iter().any(|f| f.time_ms == 20000));
    assert!(
        selected
            .windows(2)
            .all(|pair| pair[1].time_ms - pair[0].time_ms >= 10000)
    );
}
#[test]
fn long_video_counts_audio_windows_and_an_audio_only_plan_creates_empty_frames() {
    use crate::domain::settings::Settings;
    use crate::domain::source::{sha256_hex, video};
    let source = video::VideoSource {
        path: "sources/1970/01/src-video/source.md".into(),
        markdown: "---\nid: src-video\nkind: video\nsensitivity: normal\n---\n".into(),
        bytes: b"video".to_vec(),
    };
    let settings = Settings::default();
    let plan = video::plan(
        &source,
        &video::VideoAnalysis {
            duration_ms: 3_060_000,
            has_audio: true,
            candidates: Vec::new(),
        },
        &settings,
        "normal",
    )
    .unwrap();
    assert_eq!(
        plan.counts,
        video::VideoCalls {
            audio: 6,
            frames: 0,
            extract: 5
        }
    );
    assert_eq!(plan.original_sha256, sha256_hex(b"video"));
    let preview = video::apply(
        &source,
        &settings,
        video::VideoMutation::Plan {
            plan: Box::new(plan.clone()),
            confirm_digest: None,
            at: "1970-01-01T00:00:00Z".into(),
        },
    )
    .unwrap();
    let source = video::VideoSource {
        markdown: preview.markdown,
        ..source
    };
    let approved = video::apply(
        &source,
        &settings,
        video::VideoMutation::Plan {
            plan: Box::new(plan.clone()),
            confirm_digest: Some(plan.digest),
            at: "1970-01-01T00:00:00Z".into(),
        },
    )
    .unwrap();
    assert_eq!(approved.files, vec![("frames.md".into(), Vec::<u8>::new())]);
    assert!(crate::domain::source::parse_source_participants(&approved.markdown).is_ok());
}
