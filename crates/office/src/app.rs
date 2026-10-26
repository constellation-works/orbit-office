//! Composition: read one request envelope, dispatch it through the tools,
//! and return the one response envelope for the output module to write.
//! Adapters (`office-store`, `office-models`, `office-orbit`) are built here
//! and handed to `office-tools` once the first tool needs a port.
//!
//! This module knows nothing of argument parsing or rendering (STD-02 R2):
//! `main` parses and dispatches, this composes, `output` writes.

use crate::error::Result;
use std::io::Read;

/// Serve one Orbit exec call and return the payload for stdout.
///
/// A request that cannot be read is answered like an empty (unparseable)
/// one, so Orbit still receives a well-formed error envelope.
pub(crate) fn serve_exec() -> Result<String> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    Ok(office_tools::serve(&raw))
}
