//! SHA-256 content hashing. A source's original is pinned by the `sha256`
//! in its frontmatter, and capture dedupes on it, so hashing is a
//! store-level mechanism rather than a per-call-site helper.

use std::io::{ErrorKind, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Bytes read per step while hashing a file. This is the only buffer
/// `sha256_file` allocates, so its peak memory does not grow with the file.
const HASH_CHUNK_BYTES: usize = 64 * 1024;

/// Hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// Hex SHA-256 of a file's contents, streamed through the digest in
/// fixed-size chunks rather than read into memory whole.
pub fn sha256_file(path: &Path) -> Result<String> {
    let read_error = |source: std::io::Error| Error::io("read for hashing", path, source);
    let mut file = std::fs::File::open(path).map_err(read_error)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; HASH_CHUNK_BYTES];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => hasher.update(&chunk[..read]),
            Err(source) if source.kind() == ErrorKind::Interrupted => {}
            Err(source) => return Err(read_error(source)),
        }
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
