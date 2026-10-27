//! Source reads and privacy rules shared by Library adapters.

use crate::domain::endpoint::Capability;
use crate::domain::knowledge::frontmatter;
use crate::domain::settings::Settings;
use crate::{Error, Result};
use serde_json::{Value, json};

use super::{parse_clock_seconds, video::refusal};

/// Maximum raw original/frame bytes returned by source-show (512 KiB).
pub const MEDIA_LIMIT: usize = 512 * 1024;

/// The only source media selections accepted by the Library port.
#[derive(Clone, Debug)]
pub enum MediaSelection {
    /// The original recorded in source frontmatter.
    Original,
    /// A generated PNG basename, such as `0000.png`.
    Frame(String),
}

/// Bounded media read. Over-cap content has no payload, never a partial file.
pub struct SourceMedia {
    /// Source-relative file name.
    pub name: String,
    /// Recorded original MIME type, or image/png for a frame.
    pub media_type: String,
    /// File size observed by the adapter.
    pub bytes: u64,
    /// Complete bytes, or None when the file exceeds MEDIA_LIMIT.
    pub content: Option<Vec<u8>>,
}

fn fields(markdown: &str) -> Result<Vec<(String, Value)>> {
    let (header, _) = frontmatter::split(markdown)?;
    frontmatter::parse(header)
}

/// Provenance exactly as recorded by capture and processing, empty when absent.
pub fn derived_by(markdown: &str) -> Result<Value> {
    let value = fields(markdown)?
        .into_iter()
        .find(|(key, _)| key == "derived_by")
        .map_or_else(|| json!({}), |(_, value)| value);
    let map = value
        .as_object()
        .ok_or_else(|| refusal("invalid_source", "derived_by must be a mapping"))?;
    for (key, provenance) in map {
        if !matches!(key.as_str(), "transcription" | "extraction" | "frames") {
            return Err(refusal("invalid_source", "unsupported derived_by field"));
        }
        if key == "transcription" && provenance == "vtt-import" {
            continue;
        }
        let entry = provenance
            .as_object()
            .ok_or_else(|| refusal("invalid_source", "invalid derived_by provenance"))?;
        if entry.len() != 2
            || !entry
                .get("route")
                .and_then(Value::as_str)
                .is_some_and(|route| {
                    matches!(
                        route,
                        "hosted" | "private:company" | "private:provider" | "private:own_hardware"
                    )
                })
            || !entry.get("model").is_some_and(Value::is_string)
        {
            return Err(refusal(
                "invalid_source",
                "derived_by requires a recorded route and model",
            ));
        }
    }
    Ok(value)
}

/// Parse the chapters.md format emitted by office.extract as structured data.
/// A malformed sidecar refuses rather than presenting an incomplete chapter list.
pub fn chapters(markdown: &str) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut pending = None;
    for line in markdown.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(heading) = line.strip_prefix("## Chapter ") {
            if pending.is_some() {
                return Err(refusal("invalid_chapters", "chapter lacks a time anchor"));
            }
            let (number, title) = heading
                .split_once(": ")
                .ok_or_else(|| refusal("invalid_chapters", "invalid chapter heading"))?;
            let number = number
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| refusal("invalid_chapters", "invalid chapter number"))?;
            let (title, label) = title
                .rsplit_once(" [")
                .and_then(|(title, label)| label.strip_suffix(']').map(|label| (title, label)))
                .filter(|(title, label)| !title.is_empty() && matches!(*label, "core" | "tangent"))
                .ok_or_else(|| refusal("invalid_chapters", "invalid chapter title or label"))?;
            pending = Some((number, title.to_string(), label.to_string()));
        } else if let Some((number, title, label)) = pending.take() {
            let stamp = line
                .strip_prefix('[')
                .and_then(|s| s.split_once("](#t="))
                .and_then(|(stamp, target)| {
                    (target.strip_suffix(')') == Some(stamp)).then_some(stamp)
                })
                .ok_or_else(|| refusal("invalid_chapters", "invalid chapter time anchor"))?;
            let start = parse_clock_seconds(stamp)
                .ok_or_else(|| refusal("invalid_chapters", "invalid chapter timestamp"))?;
            result.push(json!({"number":number,"title":title,"label":label,"start":start,"anchor":format!("#t={stamp}")}));
        } else {
            return Err(refusal("invalid_chapters", "unsupported chapter data"));
        }
    }
    if pending.is_some() {
        return Err(refusal("invalid_chapters", "chapter lacks a time anchor"));
    }
    Ok(result)
}

/// Resolve a source-relative allowlisted name and enforce the current private
/// route's recorded text and media capabilities before reading any payload.
pub fn media_file(
    markdown: &str,
    selection: &MediaSelection,
    settings: &Settings,
) -> Result<(String, String)> {
    let fields = fields(markdown)?;
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    };
    let sensitivity = field("sensitivity")
        .map_or(Some("normal"), Value::as_str)
        .filter(|s| matches!(*s, "normal" | "private"))
        .ok_or_else(|| refusal("invalid_source", "invalid sensitivity"))?;
    let (name, mime) = match selection {
        MediaSelection::Original => {
            let original = field("original")
                .ok_or_else(|| refusal("original_unavailable", "source has no original"))?;
            let name = original["file"]
                .as_str()
                .filter(|name| {
                    name.strip_prefix("original.").is_some_and(|extension| {
                        !extension.is_empty()
                            && extension.bytes().all(|b| b.is_ascii_alphanumeric())
                    })
                })
                .ok_or_else(|| refusal("unsafe_path", "invalid original file name"))?;
            let mime = original["media_type"]
                .as_str()
                .ok_or_else(|| refusal("invalid_source", "original lacks media_type"))?;
            (name.to_string(), mime.to_string())
        }
        MediaSelection::Frame(name) => (frame_file(name)?, "image/png".into()),
    };
    if sensitivity == "private" {
        let route = settings.open_weights.as_ref().ok_or_else(|| {
            refusal(
                "private_endpoint_unconfigured",
                "private media requires a configured private endpoint",
            )
        })?;
        if route.verified_at.is_none() || !route.accepts(Capability::Text) {
            return Err(refusal(
                "private_endpoint_unverified",
                "private endpoint has not passed its text check",
            ));
        }
        let required: &[Capability] = if mime.starts_with("video/") {
            &[Capability::Image, Capability::Audio]
        } else if mime.starts_with("audio/") {
            &[Capability::Audio]
        } else if mime.starts_with("image/") {
            &[Capability::Image]
        } else if mime.starts_with("text/") {
            &[]
        } else {
            return Err(refusal(
                "private_media_unsupported",
                "private original media_type is unsupported",
            ));
        };
        for capability in required {
            if !route.accepts(*capability) {
                return Err(refusal(
                    "private_media_unsupported",
                    &format!(
                        "private endpoint lacks recorded {} capability",
                        capability.as_str()
                    ),
                ));
            }
        }
    }
    Ok((name, mime))
}

/// Resolve only generated PNG basenames; no caller-controlled directory path.
pub fn frame_file(name: &str) -> Result<String> {
    if !name.strip_suffix(".png").is_some_and(|number| {
        (4..=10).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit())
    }) {
        return Err(Error::InvalidArgument(
            "frame must be a generated PNG basename such as 0000.png".into(),
        ));
    }
    Ok(format!("frames/{name}"))
}

/// Change only the sensitivity line, preserving all unrelated metadata/body.
/// Called with the latest source under the adapter's writer lock.
pub fn with_sensitivity(markdown: &str, sensitivity: &str) -> Result<String> {
    if !matches!(sensitivity, "normal" | "private") {
        return Err(Error::InvalidArgument(
            "sensitivity must be normal or private".into(),
        ));
    }
    let fields = fields(markdown)?;
    let old = fields.iter().find(|(key, _)| key == "sensitivity");
    let current = old
        .map_or(Some("normal"), |(_, v)| v.as_str())
        .filter(|s| matches!(*s, "normal" | "private"))
        .ok_or_else(|| refusal("invalid_source", "invalid sensitivity"))?;
    if current == "private" && sensitivity == "normal" {
        return Err(refusal(
            "sensitivity_downgrade_refused",
            "sensitivity cannot change from private to normal",
        ));
    }
    if current == sensitivity {
        return Ok(markdown.to_string());
    }
    // split has validated the delimiters and parse has refused duplicate keys.
    let (header, _) = frontmatter::split(markdown)?;
    let next = if old.is_some() {
        header
            .split_inclusive('\n')
            .map(|line| {
                if line
                    .split_once(':')
                    .is_some_and(|(key, _)| key.trim_end() == "sensitivity")
                {
                    format!(
                        "sensitivity: {sensitivity}{}",
                        if line.ends_with('\n') { "\n" } else { "" }
                    )
                } else {
                    line.to_string()
                }
            })
            .collect::<String>()
    } else {
        format!("sensitivity: {sensitivity}\n{header}")
    };
    Ok(markdown.replacen(header, &next, 1))
}

/// Standard padded base64, used only after the bounded adapter read.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = usize::from(chunk[0]);
        let b = usize::from(*chunk.get(1).unwrap_or(&0));
        let c = usize::from(*chunk.get(2).unwrap_or(&0));
        for digit in [
            DIGITS[a >> 2],
            DIGITS[((a & 3) << 4) | (b >> 4)],
            if chunk.len() > 1 {
                DIGITS[((b & 15) << 2) | (c >> 6)]
            } else {
                b'='
            },
            if chunk.len() > 2 {
                DIGITS[c & 63]
            } else {
                b'='
            },
        ] {
            result.push(char::from(digit));
        }
    }
    result
}

/// Timeline entries from the exact frames.md format published by video processing.
pub fn frames(markdown: &str) -> Result<Vec<Value>> {
    markdown.lines().filter(|line| !line.trim().is_empty()).enumerate().map(|(index, line)| {
        let invalid = || refusal("invalid_frames", "invalid frame timeline");
        let (stamp, rest) = line.trim().strip_prefix('[').and_then(|s| s.split_once("] ")).ok_or_else(invalid)?;
        let time = parse_clock_seconds(stamp).ok_or_else(invalid)?;
        let suffix = format!(" [frame](#frame={stamp})");
        let text = rest.strip_suffix(&suffix).ok_or_else(invalid)?;
        Ok(json!({"time":time,"anchor":format!("#frame={stamp}"),"file":format!("{index:04}.png"),"text":text}))
    }).collect()
}

/// Cited warnings already extracted into the bootstrap digest, with no new inference.
pub fn gotchas(text: &str) -> Result<Vec<Value>> {
    let digest: Value = serde_json::from_str(text)
        .map_err(|_| refusal("invalid_source_digest", "invalid source digest"))?;
    crate::domain::bootstrap::digest(&digest)?;
    let mut gotchas = Vec::new();
    for claim in digest["claims"].as_array().into_iter().flatten() {
        if let Some(text) = claim["text"]
            .as_str()
            .and_then(|s| s.strip_prefix("Gotcha: "))
        {
            if !claim["cites"]
                .as_array()
                .is_some_and(|cites| !cites.is_empty() && cites.iter().all(Value::is_string))
            {
                return Err(refusal(
                    "invalid_source_digest",
                    "gotcha lacks source citations",
                ));
            }
            gotchas.push(json!({"text":text,"cites":claim["cites"]}));
        }
    }
    Ok(gotchas)
}
