//! Sources: ids, kinds, anchors, versions, dedupe and retention (specs/library.md).
//!
//! The meeting body is the first rule this module owns. Capture, ids and
//! retention stay with the task that builds `office.transcribe`.

/// Source display metadata, bounded media and monotonic privacy updates.
pub mod access;
/// KT video planning and processing state.
pub mod video;

use std::time::{SystemTime, UNIX_EPOCH};

use office_common::Sensitivity;

use crate::ports::{Transcript, TranscriptChunk, TranscriptSegment};
use crate::{Error, Result};

/// Meeting source body from a transcript (specs/library.md, Sources).
///
/// One paragraph per reported speaker turn, prefixed `[hh:mm:ss] Speaker:`,
/// with paragraphs separated by a blank line. The timestamp is the turn's
/// start, truncated to whole seconds. A turn with no speaker is labeled
/// `Speaker`. When the transcript reports no turns, the body is the
/// transcript text, unchanged.
///
/// A split recording carries [`TranscriptChunk`]s in order. A chunk that
/// reported no turns keeps its text, unchanged, in the position it occupies,
/// as its own paragraph between the surrounding turns. Adjacent text-only
/// chunks are still separate paragraphs. When no chunk reported turns, the
/// body stays the transcript text (chunks joined with newlines), not a
/// blank-line join. A transcript with no chunk list uses `segments`, or
/// `text` when that is empty.
#[must_use]
pub fn meeting_body(transcript: &Transcript) -> String {
    if !has_ordered_turns(transcript) {
        return turns_or_text(transcript);
    }
    let mut blocks = Vec::new();
    for chunk in &transcript.chunks {
        match chunk {
            TranscriptChunk::Text(text) => {
                if !text.is_empty() {
                    blocks.push(text.clone());
                }
            }
            TranscriptChunk::Turns(segments) => {
                for segment in segments {
                    blocks.push(format_turn(segment));
                }
            }
        }
    }
    blocks.join("\n\n")
}

fn has_ordered_turns(transcript: &Transcript) -> bool {
    transcript
        .chunks
        .iter()
        .any(|chunk| matches!(chunk, TranscriptChunk::Turns(segments) if !segments.is_empty()))
}

fn turns_or_text(transcript: &Transcript) -> String {
    if transcript.segments.is_empty() {
        transcript.text.clone()
    } else {
        format_turns(&transcript.segments)
    }
}

fn format_turns(segments: &[TranscriptSegment]) -> String {
    let mut body = String::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            body.push_str("\n\n");
        }
        body.push_str(&format_turn(segment));
    }
    body
}

fn format_turn(segment: &TranscriptSegment) -> String {
    let speaker = segment.speaker.as_deref().unwrap_or("Speaker");
    format!(
        "[{}] {speaker}: {}",
        clock_time(segment.start_ms),
        segment.text
    )
}

/// Seconds from an `hh:mm:ss` stamp, in the form `clock_time` writes and
/// the knowledge citation validator accepts.
pub(crate) fn parse_clock_seconds(stamp: &str) -> Option<u64> {
    let mut parts = stamp.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || h.len() < 2
        || [m, s].iter().any(|p| p.len() != 2)
        || ![h, m, s]
            .iter()
            .all(|p| p.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let (h, m, s): (u64, u64, u64) = (h.parse().ok()?, m.parse().ok()?, s.parse().ok()?);
    checked_clock_seconds(h, m, s)
}

fn checked_clock_seconds(h: u64, m: u64, s: u64) -> Option<u64> {
    if m >= 60 || s >= 60 {
        return None;
    }
    h.checked_mul(3_600)?
        .checked_add(m.checked_mul(60)?)?
        .checked_add(s)
}

/// `hh:mm:ss` from a millisecond offset. Hours grow past 99 instead of wrapping.
fn clock_time(start_ms: u64) -> String {
    let total_seconds = start_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

// ---------------------------------------------------------------------------
// Source identity, kinds, anchors, conversation entries and VTT import.
// ---------------------------------------------------------------------------

/// Source kinds (specs/library.md, Sources), plus `video` (specs/kt-videos.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Documented item: Video,.
    Video,
    /// Documented item: Meeting,.
    Meeting,
    /// Documented item: Chat,.
    Chat,
    /// Documented item: Email,.
    Email,
    /// Documented item: Doc,.
    Doc,
    /// Documented item: Receipt,.
    Receipt,
    /// Documented item: Bill,.
    Bill,
    /// Documented item: Conversation,.
    Conversation,
    /// Documented item: Note,.
    Note,
}

impl SourceKind {
    /// Canonical token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Meeting => "meeting",
            Self::Chat => "chat",
            Self::Email => "email",
            Self::Doc => "doc",
            Self::Receipt => "receipt",
            Self::Bill => "bill",
            Self::Conversation => "conversation",
            Self::Note => "note",
        }
    }
    /// Parse a canonical token.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "video" => Some(Self::Video),
            "meeting" => Some(Self::Meeting),
            "chat" => Some(Self::Chat),
            "email" => Some(Self::Email),
            "doc" => Some(Self::Doc),
            "receipt" => Some(Self::Receipt),
            "bill" => Some(Self::Bill),
            "conversation" => Some(Self::Conversation),
            "note" => Some(Self::Note),
            _ => None,
        }
    }
}

/// The seven decision-log entry kinds (specs/sessions.md). Criterion 8 of
/// ORB-18714 supersedes the six-kind list in its description: `fact` is in.
pub const CONVERSATION_KINDS: &[&str] = &[
    "decision",
    "direction",
    "fact",
    "action",
    "open",
    "resolved",
    "handoff",
];

/// Whether a token is one of the seven entry kinds.
#[must_use]
pub fn is_conversation_kind(token: &str) -> bool {
    CONVERSATION_KINDS.contains(&token)
}

/// Documented item: fn invalid_entry_kind.
pub fn invalid_entry_kind(token: &str) -> Error {
    Error::Library {
        code: "invalid_conversation_entry_kind",
        message: format!(
            "conversation entry kind {token:?} is not one of: {}",
            CONVERSATION_KINDS.join(", ")
        ),
    }
}

/// Lowercase hyphen slug from free text; empty input yields `source`.
#[must_use]
pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = true;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "source".to_string()
    } else {
        slug
    }
}

/// `src-YYYYMMDD-<slug>` before uniqueness suffixing. `captured_at` is RFC 3339.
#[must_use]
pub fn source_id_base(captured_at: &str, title: &str) -> String {
    let date: String = captured_at.chars().filter(|c| *c != '-').take(8).collect();
    let date = if date.len() == 8 {
        date
    } else {
        "19700101".to_string()
    };
    format!("src-{date}-{}", slugify(title))
}

/// First free id: base, then `-2`, `-3`, ...
#[must_use]
pub fn unique_source_id(base: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(base) {
        return base.to_string();
    }
    let mut n = 2_u32;
    loop {
        let candidate = format!("{base}-{n}");
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Normalized pasted text for dedupe and storage: LF endings, no trailing
/// whitespace per line, no leading/trailing blank space.
#[must_use]
pub fn normalize_text(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unified.lines().map(|l| l.trim_end()).collect();
    lines.join("\n").trim().to_string()
}

/// Hex SHA-256 of bytes.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    office_common::fs::hash::sha256_hex(bytes)
}

/// RFC 3339 UTC (`2026-10-27T02:00:00+00:00`) from a system time.
#[must_use]
pub fn format_rfc3339_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).unwrap_or(0);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = i64::try_from(yoe).unwrap_or(0) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = u64::try_from(y_adj - era * 400).unwrap_or(0);
    let mp = u64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + i64::try_from(doe).unwrap_or(0) - 719_468
}

/// Date `days` after the date part of `captured_at`, as `YYYY-MM-DD`.
#[must_use]
pub fn retention_until(captured_at: &str, days: u32) -> String {
    let mut parts = captured_at.split('T').next().unwrap_or("").split('-');
    let (Ok(y), Ok(m), Ok(d)) = (
        parts.next().unwrap_or("").parse::<i64>(),
        parts.next().unwrap_or("").parse::<u32>(),
        parts.next().unwrap_or("").parse::<u32>(),
    ) else {
        return String::new();
    };
    let (yy, mm, dd) = civil_from_days(days_from_civil(y, m, d) + i64::from(days));
    format!("{yy:04}-{mm:02}-{dd:02}")
}

/// A parsed source anchor: time, lines, slide or page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// One presentation slide and its notes.
    Slide {
        /// Slide number, 1-based in presentation order.
        number: usize,
    },
    /// One converted PDF page.
    Page {
        /// Page number, 1-based.
        number: usize,
    },
    /// Documented item: Time { seconds.
    /// A `#t=` anchor.
    Time {
        /// Seconds into the recording.
        seconds: u64,
    },
    /// Documented item: Lines { start.
    /// A `#L` anchor.
    Lines {
        /// First line, 1-based.
        start: usize,
        /// Last line, 1-based.
        end: usize,
    },
}

/// Documented item: fn anchor_invalid.
pub fn anchor_invalid(raw: &str) -> Error {
    Error::Library {
        code: "anchor_invalid",
        message: format!(
            "anchor {raw:?} is malformed; use #t=hh:mm:ss, #L<start>[-<end>], #slide=<n> or #page=<n>"
        ),
    }
}

/// Documented item: fn anchor_out_of_range.
pub fn anchor_out_of_range(raw: &str) -> Error {
    Error::Library {
        code: "anchor_out_of_range",
        message: format!("anchor {raw:?} is outside this source"),
    }
}

/// Parse an anchor, with or without the leading `#`.
pub fn parse_anchor(raw: &str) -> Result<Anchor> {
    let text = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    for (key, slide) in [("slide=", true), ("page=", false)] {
        if let Some(rest) = text.strip_prefix(key) {
            let number = rest
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0 && rest.bytes().all(|c| c.is_ascii_digit()))
                .ok_or_else(|| anchor_invalid(raw))?;
            return Ok(if slide {
                Anchor::Slide { number }
            } else {
                Anchor::Page { number }
            });
        }
    }
    if let Some(rest) = text.strip_prefix("t=") {
        let mut parts = rest.split(':');
        let (Some(h), Some(m), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(anchor_invalid(raw));
        };
        let parse = |p: &str| p.parse::<u64>().map_err(|_| anchor_invalid(raw));
        let (h, m, s) = (parse(h)?, parse(m)?, parse(s)?);
        return Ok(Anchor::Time {
            seconds: checked_clock_seconds(h, m, s).ok_or_else(|| anchor_invalid(raw))?,
        });
    }
    if let Some(rest) = text.strip_prefix('L') {
        let (start, end) = match rest.split_once('-') {
            Some((a, b)) => (
                a.parse::<usize>().map_err(|_| anchor_invalid(raw))?,
                b.parse::<usize>().map_err(|_| anchor_invalid(raw))?,
            ),
            None => {
                let n = rest.parse::<usize>().map_err(|_| anchor_invalid(raw))?;
                (n, n)
            }
        };
        if start == 0 || end < start {
            return Err(anchor_invalid(raw));
        }
        return Ok(Anchor::Lines { start, end });
    }
    Err(anchor_invalid(raw))
}

/// The span an anchor names in `body`.
///
/// A time anchor names the speaker turn in progress at that time: the
/// latest turn starting at or before it. It resolves for every time from
/// the first turn's timestamp to the last's, matching the citation
/// contract `resolve_anchor` validates; times outside that range are
/// out of range.
pub fn anchor_span(body: &str, anchor: Anchor, raw: &str) -> Result<String> {
    match anchor {
        Anchor::Slide { number } => section_span(body, "slide", number, raw),
        Anchor::Page { number } => section_span(body, "page", number, raw),
        Anchor::Lines { start, end } => {
            let lines: Vec<&str> = body.lines().collect();
            if start > lines.len() || end > lines.len() {
                return Err(anchor_out_of_range(raw));
            }
            Ok(lines[start - 1..end].join("\n"))
        }
        Anchor::Time { seconds } => {
            // Turns are `[hh:mm:ss] Speaker: text` lines, the same starts
            // `resolve_anchor` accepts. A turn runs from its timestamped
            // line to the next timestamped line in the same paragraph
            // (blank lines separate paragraphs), so single-newline bodies
            // resolve the same turns citation validation accepts.
            let mut turns: Vec<(u64, String)> = Vec::new();
            for para in body.split("\n\n") {
                let lines: Vec<&str> = para.lines().collect();
                let mut starts: Vec<(usize, u64)> = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    let trimmed = line.trim_start();
                    let Some(rest) = trimmed.strip_prefix('[') else {
                        continue;
                    };
                    let Some((stamp, _)) = rest.split_once(']') else {
                        continue;
                    };
                    if let Some(start) = parse_clock_seconds(stamp) {
                        starts.push((index, start));
                    }
                }
                for (position, (index, start)) in starts.iter().enumerate() {
                    let end = if position + 1 < starts.len() {
                        starts[position + 1].0
                    } else {
                        lines.len()
                    };
                    turns.push((*start, lines[*index..end].join("\n")));
                }
            }
            let (Some(first), Some(last)) = (
                turns.iter().map(|(start, _)| start).min(),
                turns.iter().map(|(start, _)| start).max(),
            ) else {
                return Err(anchor_out_of_range(raw));
            };
            if seconds < *first || seconds > *last {
                return Err(anchor_out_of_range(raw));
            }
            // The turn in progress: the latest turn starting at or before
            // the requested time. `seconds >= first` guarantees one exists.
            // On a tied start the first such turn wins, as exact matching did.
            let mut current: Option<(u64, String)> = None;
            for (start, para) in turns {
                if start <= seconds && current.as_ref().is_none_or(|(best, _)| start > *best) {
                    current = Some((start, para));
                }
            }
            match current {
                Some((_, para)) => Ok(para),
                None => Err(anchor_out_of_range(raw)),
            }
        }
    }
}

fn section_span(body: &str, kind: &str, number: usize, raw: &str) -> Result<String> {
    let lines: Vec<&str> = body.lines().collect();
    fn heading(line: &str) -> Option<(usize, &str)> {
        let level = line.bytes().take_while(|ch| *ch == b'#').count();
        (level > 0 && level <= 6 && line.as_bytes().get(level) == Some(&b' '))
            .then(|| (level, line[level + 1..].trim()))
    }
    let Some((start, level)) = lines.iter().enumerate().find_map(|(index, line)| {
        let (level, title) = heading(line)?;
        let title = title.to_ascii_lowercase();
        let rest = title.strip_prefix(kind)?.strip_prefix(' ')?;
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        (digits.parse::<usize>().ok() == Some(number)).then_some((index, level))
    }) else {
        return Err(anchor_out_of_range(raw));
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| heading(line).is_some_and(|(next, _)| next <= level))
        .map_or(lines.len(), |(index, _)| index);
    Ok(lines[start..end].join("\n").trim_end().to_string())
}

/// Validate capture tags once for both the persistent and fake Library ports.
pub fn validate_capture_tags(tags: &[String]) -> Result<()> {
    if tags.len() > 64
        || tags.iter().any(|tag| {
            tag.trim().is_empty() || tag.len() > 128 || tag.chars().any(char::is_control)
        })
    {
        return Err(Error::InvalidArgument("capture tags must contain at most 64 nonempty strings, each at most 128 bytes without control characters".into()));
    }
    Ok(())
}

/// Merge capture tags into an existing source without changing its body or
/// other metadata. `None` means every requested tag is already present.
pub fn merge_source_tags(markdown: &str, tags: &[String]) -> Result<Option<String>> {
    validate_capture_tags(tags)?;
    if tags.is_empty() {
        return Ok(None);
    }
    let record = crate::domain::knowledge::DocumentRecord::from_markdown(
        std::path::Path::new("sources/source.md"),
        markdown,
    );
    let mut merged = record.tags.clone();
    for tag in tags {
        if !merged.contains(tag) {
            merged.push(tag.clone());
        }
    }
    if merged == record.tags {
        return Ok(None);
    }
    validate_capture_tags(&merged)?;
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return Err(Error::Library {
            code: "document_invalid",
            message: "source has no frontmatter for tags".into(),
        });
    };
    let Some((frontmatter, body)) = rest.split_once("\n---") else {
        return Err(Error::Library {
            code: "document_invalid",
            message: "source has no closing frontmatter fence".into(),
        });
    };
    let mut out = String::from("---\n");
    let mut in_tags = false;
    for line in frontmatter.lines() {
        if line.starts_with("tags:") {
            in_tags = true;
            continue;
        }
        if in_tags && (line.starts_with([' ', '\t']) || line.starts_with("- ")) {
            continue;
        }
        in_tags = false;
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("tags:\n");
    for tag in merged {
        out.push_str(&format!("  - {}\n", serde_json::json!(tag)));
    }
    out.push_str("---");
    out.push_str(body);
    Ok(Some(out))
}

/// One formatted decision-log line: `[<rfc3339>] <kind>: <text>`.
pub fn format_conversation_entry(captured_at: &str, kind: &str, text: &str) -> Result<String> {
    if !is_conversation_kind(kind) {
        return Err(invalid_entry_kind(kind));
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::InvalidArgument(
            "conversation entry text must not be empty".to_string(),
        ));
    }
    if text.contains('\n') {
        return Err(Error::InvalidArgument(
            "conversation entry text must be one line".to_string(),
        ));
    }
    Ok(format!("[{captured_at}] {kind}: {text}"))
}

/// Import a WebVTT transcript as meeting-style `#t=` turns. Speaker names
/// come from `<v Name>` voice spans or a leading `Name:`; cue start times
/// become the turn timestamps. Both cue endpoints must fit in `u64`
/// milliseconds; malformed or overflowing timestamps refuse the import.
/// No model call is involved.
pub fn vtt_to_body(vtt: &str) -> Result<String> {
    let mut turns: Vec<String> = Vec::new();
    let mut cue_start_ms: Option<u64> = None;
    let mut cue_lines: Vec<String> = Vec::new();
    let flush = |turns: &mut Vec<String>, start: &mut Option<u64>, lines: &mut Vec<String>| {
        if let Some(start_ms) = *start {
            let raw = lines.join(" ").trim().to_string();
            if !raw.is_empty() {
                let (speaker, text) = split_vtt_speaker(&raw);
                turns.push(format!(
                    "[{}] {}: {}",
                    clock_time(start_ms),
                    speaker.as_deref().unwrap_or("Speaker"),
                    text
                ));
            }
        }
        *start = None;
        lines.clear();
    };
    for line in vtt.lines() {
        let line = line.trim();
        if let Some((start, end)) = line.split_once("-->") {
            flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
            cue_start_ms = Some(parse_vtt_time(start)?);
            // End times do not become turn starts, but must not silently
            // accept overflow either. Cue settings follow the endpoint.
            parse_vtt_time(end.split_whitespace().next().unwrap_or(""))?;
        } else if line.is_empty() {
            flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
        } else if cue_start_ms.is_some() {
            // Cue state alone separates payload from the header, cue identifiers
            // and NOTE/STYLE blocks: those sit before a timing line or after a
            // blank line. Content checks here would drop real speech ("1234").
            cue_lines.push(line.to_string());
        }
    }
    flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
    Ok(turns.join("\n\n"))
}

fn parse_vtt_time(token: &str) -> Result<u64> {
    let token = token.trim();
    let malformed = || {
        // The token comes from transcript contents; diagnostics must not
        // echo arbitrary speech or secrets from a malformed timing line.
        Error::InvalidArgument(
            "WebVTT timestamp is malformed; use hh:mm:ss.mmm or mm:ss.mmm".into(),
        )
    };
    let (hms, frac) = token.split_once('.').unwrap_or((token, "0"));
    let nums: Vec<u64> = hms
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(malformed)?;
    let millis: u64 = frac
        .chars()
        .take(3)
        .collect::<String>()
        .parse()
        .map_err(|_| malformed())?;
    let (h, m, s) = match nums.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return Err(malformed()),
    };
    if m >= 60 || s >= 60 {
        return Err(malformed());
    }
    checked_clock_seconds(h, m, s)
        .and_then(|seconds| seconds.checked_mul(1_000))
        .and_then(|ms| ms.checked_add(millis))
        .ok_or_else(|| {
            Error::InvalidArgument(
                "WebVTT timestamp exceeds the supported u64 millisecond range".into(),
            )
        })
}

fn split_vtt_speaker(raw: &str) -> (Option<String>, String) {
    if let Some(rest) = raw.strip_prefix("<v ")
        && let Some((name, text)) = rest.split_once('>')
    {
        let text = text.strip_suffix("</v>").unwrap_or(text).trim().to_string();
        return (Some(name.trim().to_string()), text);
    }
    if let Some((name, text)) = raw.split_once(": ") {
        let words = name.split_whitespace().count();
        let name_like = (1..=2).contains(&words)
            && name.chars().count() <= 40
            && name
                .chars()
                .all(|c| c.is_alphabetic() || c == ' ' || c == '.');
        if name_like {
            return (Some(name.trim().to_string()), text.trim().to_string());
        }
    }
    (None, raw.to_string())
}

/// Parsed source frontmatter fields the index record does not carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMeta {
    /// Documented item: id.
    pub id: String,
    /// Documented item: kind.
    pub kind: String,
    /// Documented item: title.
    pub title: String,
    /// Documented item: sensitivity.
    pub sensitivity: String,
    /// Documented item: sha256.
    pub sha256: Option<String>,
    /// Documented item: bytes.
    pub bytes: Option<u64>,
    /// Documented item: media_type.
    pub media_type: Option<String>,
    /// Documented item: original_file.
    pub original_file: Option<String>,
    /// Documented item: version.
    pub version: u32,
}

/// Body of a source document (after `---` frontmatter), or the whole text.
#[must_use]
pub fn source_body(text: &str) -> String {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text.to_string();
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            after.strip_prefix('\n').unwrap_or(after).to_string()
        }
        None => text.to_string(),
    }
}

/// Read source participants using the Library's YAML subset, including
/// quoted names and block lists. Malformed metadata is refused rather than
/// silently treated as a source with no participants.
pub fn parse_source_participants(text: &str) -> Result<Vec<String>> {
    use crate::domain::knowledge::frontmatter;

    let (header, _) = frontmatter::split(text)?;
    let fields = frontmatter::parse(header)?;
    let Some((_, value)) = fields.iter().find(|(key, _)| key == "participants") else {
        return Ok(Vec::new());
    };
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| Error::InvalidFrontmatter {
                        field: "participants".into(),
                        reason: "expected a list of strings",
                    })
            })
            .collect(),
        serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_)
        | serde_json::Value::Object(_) => Err(Error::InvalidFrontmatter {
            field: "participants".into(),
            reason: "expected a list of strings",
        }),
    }
}

/// Parse the source frontmatter subset capture writes.
#[must_use]
pub fn parse_source_meta(text: &str) -> SourceMeta {
    let mut meta = SourceMeta {
        version: 1,
        ..SourceMeta::default()
    };
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return meta;
    };
    let Some(end) = rest.find("\n---") else {
        return meta;
    };
    for line in rest[..end].lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        // Only a double-quoted scalar is decoded, and only the escapes
        // `yaml_quote` writes (`\\` and `\"`), in a single pass so an
        // already-escaped sequence is never decoded twice. Unquoted
        // values (such as `original.file`) keep their backslashes.
        let unq = |v: &str| {
            let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
                return v.to_string();
            };
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(ch) = chars.next() {
                if ch != '\\' {
                    out.push(ch);
                    continue;
                }
                match chars.next() {
                    Some(escaped @ ('\\' | '"')) => out.push(escaped),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            }
            out
        };
        match key.trim() {
            "id" => meta.id = unq(value),
            "kind" => meta.kind = unq(value),
            "title" => meta.title = unq(value),
            "sensitivity" => meta.sensitivity = unq(value),
            "version" => meta.version = unq(value).parse().unwrap_or(1),
            "original" => {
                for part in value.trim_matches(|c| c == '{' || c == '}').split(',') {
                    let Some((k, v)) = part.split_once(':') else {
                        continue;
                    };
                    match k.trim() {
                        "file" => meta.original_file = Some(unq(v.trim())),
                        "sha256" => meta.sha256 = Some(unq(v.trim())),
                        "bytes" => meta.bytes = v.trim().parse().ok(),
                        "media_type" => meta.media_type = Some(unq(v.trim())),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // Processing renders parsed scalars as quoted flow YAML. Resolve original
    // structurally too, so quoted numbers and mapping keys retain their fields.
    if let Ok((header, _)) = crate::domain::knowledge::frontmatter::split(text)
        && let Ok(fields) = crate::domain::knowledge::frontmatter::parse(header)
        && let Some((_, original)) = fields.iter().find(|(key, _)| key == "original")
    {
        meta.original_file = original["file"].as_str().map(str::to_string);
        meta.sha256 = original["sha256"].as_str().map(str::to_string);
        meta.bytes = original["bytes"]
            .as_u64()
            .or_else(|| original["bytes"].as_str()?.parse().ok());
        meta.media_type = original["media_type"].as_str().map(str::to_string);
    }
    meta
}

fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("\"{escaped}\"")
}

/// Whether capture imported a WebVTT transcript, which must never invoke a model.
pub fn imported_vtt(markdown: &str) -> Result<bool> {
    let (header, _) = crate::domain::knowledge::frontmatter::split(markdown)?;
    Ok(crate::domain::knowledge::frontmatter::parse(header)?
        .iter()
        .any(|(key, value)| key == "derived_by" && value["transcription"] == "vtt-import"))
}

/// Derived files the source-processing journal may publish, never originals.
#[must_use]
pub fn processing_file_name(name: &str) -> bool {
    let numbered = |prefix: &str, suffix: &str| {
        name.strip_prefix(prefix)
            .and_then(|n| n.strip_suffix(suffix))
            .is_some_and(|n| {
                !n.is_empty() && n.len() <= 10 && n.bytes().all(|c| c.is_ascii_digit())
            })
    };
    name == "extract-result.json"
        || name == "chapters.md"
        || name == "digest.json"
        || numbered("v", ".md")
        || numbered("extract-window-", ".json")
}

/// Prepare a model result while preserving unrelated metadata. `source.md` is
/// the current view; every generated transcript has an immutable `vN.md` snapshot.
pub fn processing_markdown(
    markdown: &str,
    body: Option<&str>,
    derived: &crate::ports::DerivedBy,
) -> Result<(String, Option<String>)> {
    let (header, _) = crate::domain::knowledge::frontmatter::split(markdown)?;
    let mut fields = crate::domain::knowledge::frontmatter::parse(header)?;
    let mut provenance = fields
        .iter()
        .find(|(key, _)| key == "derived_by")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    let previous = provenance.get("transcription").is_some();
    provenance
        .as_object_mut()
        .ok_or_else(|| video::refusal("invalid_source", "derived_by must be a mapping"))?
        .insert(
            if body.is_some() {
                "transcription"
            } else {
                "extraction"
            }
            .into(),
            serde_json::json!(derived),
        );
    fields.retain(|(key, _)| key != "derived_by");
    fields.push(("derived_by".into(), provenance));
    let mut snapshot = None;
    if let Some(body) = body {
        let version = parse_source_meta(markdown).version;
        let next = if previous {
            version
                .checked_add(1)
                .ok_or_else(|| video::refusal("invalid_source", "source version overflow"))?
        } else {
            version
        };
        fields.retain(|(key, _)| key != "version" && key != "supersedes");
        fields.push(("version".into(), serde_json::json!(next)));
        fields.push((
            "supersedes".into(),
            if previous {
                serde_json::json!(format!("v{version}.md"))
            } else {
                serde_json::Value::Null
            },
        ));
        if let Some(ledger) = video::with_transcript(markdown, body)? {
            fields.retain(|(key, _)| key != "video");
            // The ledger is already encoded in the Library YAML subset.
            let rendered = fields
                .iter()
                .map(|(key, value)| format!("{key}: {}", video::flow(value)))
                .collect::<Vec<_>>()
                .join("\n");
            return Ok((
                format!("---\n{rendered}\nvideo: {ledger}\n---\n{body}"),
                Some(format!("v{next}.md")),
            ));
        }
        snapshot = Some(format!("v{next}.md"));
    }
    let rendered = fields
        .iter()
        .map(|(key, value)| format!("{key}: {}", video::flow(value)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok((
        format!(
            "---\n{rendered}\n---\n{}",
            body.map(str::to_string)
                .unwrap_or_else(|| source_body(markdown))
        ),
        snapshot,
    ))
}

/// Inputs for [`render_source`].
pub struct RenderSource<'a> {
    /// Documented item: id.
    pub id: &'a str,
    /// Documented item: kind.
    pub kind: SourceKind,
    /// Documented item: title.
    pub title: &'a str,
    /// Documented item: captured_at.
    pub captured_at: &'a str,
    /// Documented item: captured_by.
    pub captured_by: &'a str,
    /// Documented item: origin.
    pub origin: Option<&'a str>,
    /// Documented item: occurred_at.
    pub occurred_at: Option<&'a str>,
    /// Documented item: participants.
    pub participants: &'a [String],
    /// Source tags, serialized as quoted block-list items.
    pub tags: &'a [String],
    /// Documented item: sensitivity.
    pub sensitivity: Sensitivity,
    /// Documented item: original.
    pub original: Option<(&'a str, &'a str, u64, &'a str)>, // file, sha256, bytes, media_type
    /// Documented item: derived_transcription.
    pub derived_transcription: Option<&'a str>,
    /// Documented item: retention_days.
    pub retention_days: Option<u32>,
    /// Documented item: body.
    pub body: &'a str,
}

/// Render `source.md`: frontmatter plus body (specs/library.md, Sources).
#[must_use]
pub fn render_source(input: &RenderSource<'_>) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("id: {}\n", input.id));
    out.push_str(&format!("kind: {}\n", input.kind.as_str()));
    out.push_str(&format!("title: {}\n", yaml_quote(input.title)));
    if let Some(at) = input.occurred_at {
        out.push_str(&format!("occurred_at: {}\n", serde_json::json!(at)));
    }
    out.push_str(&format!("captured_at: {}\n", input.captured_at));
    out.push_str(&format!(
        "captured_by: {}\n",
        serde_json::json!(input.captured_by)
    ));
    if let Some(origin) = input.origin {
        out.push_str(&format!("origin: {}\n", yaml_quote(origin)));
    }
    if !input.participants.is_empty() {
        out.push_str(&format!(
            "participants: [{}]\n",
            input
                .participants
                .iter()
                .map(|item| serde_json::json!(item).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str(&format!("sensitivity: {}\n", input.sensitivity.as_str()));
    if !input.tags.is_empty() {
        out.push_str("tags:\n");
        for tag in input.tags {
            out.push_str(&format!("  - {}\n", serde_json::json!(tag)));
        }
    }
    match input.original {
        Some((file, sha, bytes, media)) => out.push_str(&format!(
            "original: {{ file: {file}, sha256: {sha}, bytes: {bytes}, media_type: {media} }}\n"
        )),
        None => out.push_str("original: null\n"),
    }
    if let Some(how) = input.derived_transcription {
        out.push_str(&format!("derived_by: {{ transcription: {how} }}\n"));
    }
    out.push_str("version: 1\nsupersedes: null\n");
    match input.retention_days {
        Some(days) => out.push_str(&format!(
            "retention: {{ original_until: {} }}\n",
            retention_until(input.captured_at, days)
        )),
        None => out.push_str("retention: { original_until: null }\n"),
    }
    out.push_str("---\n");
    out.push_str(input.body);
    if !input.body.is_empty() && !input.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// MIME type for a file extension, when capture knows it.
#[must_use]
pub fn media_type_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "m4a" => Some("audio/mp4"),
        "wav" => Some("audio/wav"),
        "mp3" => Some("audio/mpeg"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "pdf" => Some("application/pdf"),
        "html" | "htm" => Some("text/html"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "txt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "vtt" => Some("text/vtt"),
        _ => None,
    }
}

/// Whether an extension is one capture accepts as a `video` source
/// (specs/kt-videos.md: mp4, webm, mov and m4a).
#[must_use]
pub fn is_video_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mov" | "m4a"
    )
}
