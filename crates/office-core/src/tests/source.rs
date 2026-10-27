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
