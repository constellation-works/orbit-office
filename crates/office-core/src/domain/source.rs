//! Sources: ids, kinds, anchors, versions, dedupe and retention (specs/library.md).
//!
//! The meeting body is the first rule this module owns. Capture, ids and
//! retention stay with the task that builds `office.transcribe`.

use std::time::{SystemTime, UNIX_EPOCH};

use office_common::Sensitivity;

use crate::ports::Transcript;
use crate::{Error, Result};

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

/// Seconds from an `hh:mm:ss` stamp, in the form `clock_time` writes and
/// the knowledge citation validator accepts.
fn parse_clock_seconds(stamp: &str) -> Option<u64> {
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
    (m < 60 && s < 60).then(|| h * 3_600 + m * 60 + s)
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
        if m >= 60 || s >= 60 {
            return Err(anchor_invalid(raw));
        }
        return Ok(Anchor::Time {
            seconds: h * 3600 + m * 60 + s,
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
            // Paragraphs are `[hh:mm:ss] Speaker: text` (blank-line separated).
            let mut turns: Vec<(u64, &str)> = Vec::new();
            for para in body.split("\n\n") {
                let trimmed = para.trim_start();
                let Some(rest) = trimmed.strip_prefix('[') else {
                    continue;
                };
                let Some((stamp, _)) = rest.split_once(']') else {
                    continue;
                };
                if let Some(start) = parse_clock_seconds(stamp) {
                    turns.push((start, para));
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
            let mut current: Option<(u64, &str)> = None;
            for (start, para) in turns {
                if start <= seconds && current.is_none_or(|(best, _)| start > best) {
                    current = Some((start, para));
                }
            }
            match current {
                Some((_, para)) => Ok(para.to_string()),
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
/// become the turn timestamps. No model call is involved.
#[must_use]
pub fn vtt_to_body(vtt: &str) -> String {
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
        if line.contains("-->") {
            flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
            cue_start_ms = line.split("-->").next().and_then(parse_vtt_time);
        } else if line.is_empty() {
            flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
        } else if cue_start_ms.is_some()
            && !line.starts_with("WEBVTT")
            && !line.starts_with("NOTE")
            && !line.chars().all(|c| c.is_ascii_digit())
        {
            cue_lines.push(line.to_string());
        }
    }
    flush(&mut turns, &mut cue_start_ms, &mut cue_lines);
    turns.join("\n\n")
}

fn parse_vtt_time(token: &str) -> Option<u64> {
    let token = token.trim();
    let (hms, frac) = token.split_once('.').unwrap_or((token, "0"));
    let mut parts = hms.split(':');
    let nums: Vec<u64> = parts
        .by_ref()
        .map(|p| p.parse().ok())
        .collect::<Option<Vec<_>>>()?;
    let millis: u64 = frac.chars().take(3).collect::<String>().parse().ok()?;
    match nums.as_slice() {
        [m, s] => Some((m * 60 + s) * 1_000 + millis),
        [h, m, s] => Some((h * 3600 + m * 60 + s) * 1_000 + millis),
        _ => None,
    }
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
        let unq = |v: &str| {
            v.strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v)
                .to_string()
        };
        match key.trim() {
            "id" => meta.id = unq(value),
            "kind" => meta.kind = unq(value),
            "title" => meta.title = unq(value),
            "sensitivity" => meta.sensitivity = unq(value),
            "version" => meta.version = value.parse().unwrap_or(1),
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
    meta
}

fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("\"{escaped}\"")
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
