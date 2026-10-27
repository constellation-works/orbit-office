//! Optional poppler adapter using the shared process-group supervisor.

use std::io::Write;
use std::process::Command;

use crate::{Error, Result};

pub(super) fn convert(bytes: &[u8]) -> Result<String> {
    let scratch = tempfile::tempdir()
        .map_err(|e| Error::io("create converter directory", "temporary directory", e))?;
    let mut input = tempfile::NamedTempFile::new_in(scratch.path())
        .map_err(|e| Error::io("create pinned PDF", scratch.path(), e))?;
    input
        .write_all(bytes)
        .map_err(|e| Error::io("write pinned PDF", input.path(), e))?;
    let mut command = Command::new("pdftotext");
    command
        .args(["-layout", "-enc", "UTF-8"])
        .arg(input.path())
        .arg("-")
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", scratch.path())
        .env("TMPDIR", scratch.path())
        .env("LC_ALL", "C");
    let output =
        office_common::process::run_captured(&mut command, scratch.path()).map_err(|source| {
            if let office_common::Error::ProcessSpawn { source: ref io, .. } = source
                && io.kind() == std::io::ErrorKind::NotFound
            {
                Error::ConverterUnavailable {
                    program: "pdftotext",
                }
            } else {
                Error::ConversionProcess {
                    program: "pdftotext",
                    source,
                }
            }
        })?;
    if !output.status.success() {
        return Err(Error::ConversionFailed {
            program: "pdftotext",
            exit_code: output.status.code(),
        });
    }
    let text = super::utf8(&output.stdout, "PDF converter output")?;
    // Poppler's form-feed terminates every page, including a final empty page.
    let pages: Vec<&str> = text
        .strip_suffix('\u{c}')
        .unwrap_or(text)
        .split('\u{c}')
        .collect();
    let has_text = pages.iter().any(|page| !page.trim().is_empty());
    let mut body = String::new();
    for (index, page) in pages.iter().enumerate() {
        if index > 0 {
            body.push_str("\n\n");
        }
        body.push_str(&format!(
            "# Page {}\n\n{}",
            index + 1,
            page.trim_matches(['\r', '\n'])
        ));
    }
    if !has_text {
        body.push_str("\n\n> needs_ocr: no text was found during document conversion.");
    }
    Ok(body)
}
