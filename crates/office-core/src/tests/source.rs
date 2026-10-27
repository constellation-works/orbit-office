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
fn vtt_import_keeps_speakers_and_cue_times() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Jane>Hi there.</v>\n\n00:01:05.500 --> 00:01:07.000\nDaniel: Next point.\n";
    assert_eq!(
        vtt_to_body(vtt),
        "[00:00:01] Jane: Hi there.\n\n[00:01:05] Daniel: Next point."
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
