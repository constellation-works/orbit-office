//! SHA-256 content hashing. A source's original is pinned by the `sha256`
//! in its frontmatter, and capture dedupes on it, so hashing is a
//! store-level mechanism rather than a per-call-site helper.

use sha2::{Digest, Sha256};
use std::path::Path;

use crate::{Error, Result};

/// Hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex SHA-256 of a file's contents.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io("read for hashing", path, e))?;
    Ok(sha256_hex(&bytes))
}
