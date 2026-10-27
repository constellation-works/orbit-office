//! Meeting source body built from a transcript (specs/library.md).

use crate::domain::source::meeting_body;
use crate::ports::{Transcript, TranscriptSegment};

fn turn(speaker: Option<&str>, start_ms: u64, text: &str) -> TranscriptSegment {
    TranscriptSegment {
        speaker: speaker.map(str::to_string),
        start_ms,
        end_ms: start_ms + 1_000,
        text: text.to_string(),
    }
}

#[test]
fn meeting_body_writes_one_paragraph_per_speaker_turn() {
    let transcript = Transcript {
        text: "ignored when turns are present".into(),
        segments: vec![
            turn(Some("Jane"), 0, "We should renew."),
            turn(None, 751_000, "Agreed."),
            turn(Some("Daniel"), 360_000_000, "Next year."),
        ],
    };
    assert_eq!(
        meeting_body(&transcript),
        "[00:00:00] Jane: We should renew.\n\n[00:12:31] Speaker: Agreed.\n\n[100:00:00] Daniel: Next year."
    );
}

#[test]
fn meeting_body_keeps_text_when_no_turns_are_reported() {
    let transcript = Transcript {
        text: "plain transcript".into(),
        segments: Vec::new(),
    };
    assert_eq!(meeting_body(&transcript), "plain transcript");
    assert_eq!(meeting_body(&Transcript::default()), "");
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
fn vtt_import_keeps_speakers_and_cue_times() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Jane>Hi there.</v>\n\n00:01:05.500 --> 00:01:07.000\nDaniel: Next point.\n";
    assert_eq!(
        vtt_to_body(vtt),
        "[00:00:01] Jane: Hi there.\n\n[00:01:05] Daniel: Next point."
    );
}

#[test]
fn vtt_import_keeps_numeric_and_keyword_prefixed_cue_text() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n1234\n\n00:00:03.000 --> 00:00:04.000\nNOTE: the answer\n\n00:00:05.000 --> 00:00:06.000\nDaniel: It is\n2026\n\n00:00:07.000 --> 00:00:08.000\nWEBVTT is the format\n";
    assert_eq!(
        vtt_to_body(vtt),
        "[00:00:01] Speaker: 1234\n\n[00:00:03] NOTE: the answer\n\n[00:00:05] Daniel: It is 2026\n\n[00:00:07] Speaker: WEBVTT is the format"
    );
}

#[test]
fn vtt_import_skips_header_cue_identifiers_and_comment_blocks() {
    let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE producer comment\nthat spans lines\n\nSTYLE\n::cue { color: red }\n\n7\n00:00:01.000 --> 00:00:02.000\n<v Jane>Hi there.</v>\n\nNOTE editor aside\n\n8\n00:00:03.000 --> 00:00:04.000\nDaniel: Next point.\n";
    assert_eq!(
        vtt_to_body(vtt),
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
