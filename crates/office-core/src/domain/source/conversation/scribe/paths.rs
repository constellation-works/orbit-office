//! Library-relative SCR path validation.

use std::path::Path;

use super::{CLASH_SUFFIX, SLUG_MAX, refusal};
use crate::Result;

fn valid_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("SCR-") else {
        return false;
    };
    let parts: Vec<_> = rest.split('-').collect();
    if let [date, time, session] = parts.as_slice()
        && date.len() == 8
        && date.bytes().all(|b| b.is_ascii_digit())
        && time.len() == 4
        && time.bytes().all(|b| b.is_ascii_digit())
        && session.len() == 6
        && session
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return true;
    }
    let bytes = rest.as_bytes();
    bytes.len() > 8
        && bytes.len() <= 8 + SLUG_MAX + 1 + CLASH_SUFFIX
        && bytes[..2].iter().all(u8::is_ascii_digit)
        && bytes[2] == b'-'
        && bytes[3..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !rest.ends_with('-')
}

/// Refuse anything but `sources/conversation/<yyyy>/<mm>/SCR-….md`.
pub fn validate_scr_path(relative: &Path) -> Result<()> {
    let text = relative.to_str().unwrap_or_default();
    let parts: Vec<&str> = text.split('/').collect();
    let ok = parts.len() == 5
        && parts[0] == "sources"
        && parts[1] == "conversation"
        && parts[2].len() == 4
        && parts[2].bytes().all(|b| b.is_ascii_digit())
        && parts[3].len() == 2
        && parts[3].bytes().all(|b| b.is_ascii_digit())
        && parts[4].strip_suffix(".md").is_some_and(valid_id);
    if ok {
        Ok(())
    } else {
        Err(refusal(
            "scribe_path_invalid",
            "SCR files live at sources/conversation/<yyyy>/<mm>/SCR-<dd-hhmm>-<slug>.md or SCR-<yyyymmdd>-<hhmm>-<session6>.md",
        ))
    }
}

/// True for a Library path that names an SCR file.
#[must_use]
pub fn is_scr_path(relative: &str) -> bool {
    validate_scr_path(Path::new(relative)).is_ok()
}
