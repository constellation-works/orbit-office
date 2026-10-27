//! Sources: ids, kinds, anchors, versions, dedupe and retention (specs/library.md).
//!
//! The meeting body is the first rule this module owns. Capture, ids and
//! retention stay with the task that builds `office.transcribe`.

use crate::ports::Transcript;

/// Meeting source body from a transcript (specs/library.md, Sources).
///
/// One paragraph per reported speaker turn, prefixed `[hh:mm:ss] Speaker:`,
/// with paragraphs separated by a blank line. The timestamp is the turn's
/// start, truncated to whole seconds. A turn with no speaker is labeled
/// `Speaker`. When the transcript reports no turns, the body is the
/// transcript text, unchanged.
#[must_use]
pub fn meeting_body(transcript: &Transcript) -> String {
    if transcript.segments.is_empty() {
        return transcript.text.clone();
    }
    let mut body = String::new();
    for (index, segment) in transcript.segments.iter().enumerate() {
        if index > 0 {
            body.push_str("\n\n");
        }
        let speaker = segment.speaker.as_deref().unwrap_or("Speaker");
        body.push('[');
        body.push_str(&clock_time(segment.start_ms));
        body.push_str("] ");
        body.push_str(speaker);
        body.push_str(": ");
        body.push_str(&segment.text);
    }
    body
}

/// `hh:mm:ss` from a millisecond offset. Hours grow past 99 instead of wrapping.
fn clock_time(start_ms: u64) -> String {
    let total_seconds = start_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
