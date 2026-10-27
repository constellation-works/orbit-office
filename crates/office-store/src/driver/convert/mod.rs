//! Bounded document conversion. Complete before acquiring the Library writer.
//! Office packages are read in memory, never extracted. PDF receives pinned bytes.

mod html;
mod office_xml;
mod pdf;

use crate::{Error, Result};

pub(crate) const INPUT_LIMIT: u64 = 32 * 1024 * 1024;
const TEXT_LIMIT: usize = 8 * 1024 * 1024;

pub(crate) fn is_document(extension: &str) -> bool {
    matches!(
        extension,
        "html" | "htm" | "docx" | "pptx" | "pdf" | "md" | "txt"
    )
}

/// None for media that another capture path owns. Empty converted documents get
/// an explicit OCR note, rather than becoming silently empty sources.
pub(crate) fn document(extension: &str, bytes: &[u8]) -> Result<Option<String>> {
    if !is_document(extension) {
        return Ok(None);
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > INPUT_LIMIT {
        return Err(Error::ConversionLimit {
            limit: "32 MiB document input",
        });
    }
    let body = match extension {
        "html" | "htm" => html::convert(utf8(bytes, "HTML")?)?,
        "docx" => office_xml::docx(bytes)?,
        "pptx" => office_xml::pptx(bytes)?,
        "pdf" => pdf::convert(bytes)?,
        "md" | "txt" => utf8(bytes, "text")?.to_string(),
        _ => return Ok(None),
    };
    if body.len() > TEXT_LIMIT {
        return Err(Error::ConversionLimit {
            limit: "8 MiB converted text",
        });
    }
    Ok(Some(if body.trim().is_empty() {
        "> needs_ocr: no text was found during document conversion.".to_string()
    } else {
        body
    }))
}

fn utf8<'a>(bytes: &'a [u8], format: &'static str) -> Result<&'a str> {
    std::str::from_utf8(bytes).map_err(|_| invalid(format, "text is not UTF-8"))
}

fn invalid(format: &'static str, reason: &'static str) -> Error {
    Error::DocumentInvalid { format, reason }
}
