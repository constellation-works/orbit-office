//! Filesystem mechanics: path safety and atomic writes.

use std::path::Path;

use crate::Error;
use crate::fs::atomic::write_atomic;
use crate::fs::hash::{sha256_file, sha256_hex};
use crate::fs::path_safety::{normalize_path, validate_path_stem};

#[test]
fn path_stems_accept_slugs_and_refuse_traversal() {
    assert!(validate_path_stem("src-20261028-acme-qbr", "source").is_ok());
    assert!(validate_path_stem("../escape", "source").is_err());
    assert!(validate_path_stem("a/b", "source").is_err());
    assert!(validate_path_stem(".hidden", "source").is_err());
    assert!(validate_path_stem("", "source").is_err());
}

#[test]
fn normalize_path_preserves_leading_parent_traversals() {
    // Every input names components that do not exist on disk, so the
    // lexical arm under test runs instead of the `canonicalize` shortcut.
    assert_eq!(normalize_path(Path::new("../a")), Path::new("../a"));
    assert_eq!(normalize_path(Path::new("../../x")), Path::new("../../x"));
    assert_eq!(normalize_path(Path::new("a/../../b")), Path::new("../b"));
}

#[test]
fn normalize_path_still_resolves_dot_and_interior_dotdot() {
    assert_eq!(normalize_path(Path::new("a/./b")), Path::new("a/b"));
    assert_eq!(normalize_path(Path::new("./a")), Path::new("a"));
    assert_eq!(normalize_path(Path::new("a/b/../c")), Path::new("a/c"));
    assert_eq!(normalize_path(Path::new("/a/../b")), Path::new("/b"));
    assert_eq!(normalize_path(Path::new("/../a")), Path::new("/a"));
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested/page.md");
    write_atomic(&path, b"one").expect("first write");
    write_atomic(&path, b"two").expect("replace");
    assert_eq!(std::fs::read_to_string(&path).expect("read back"), "two");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
        .expect("list")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

#[test]
fn sha256_matches_a_known_vector() {
    // SHA-256 of the empty input.
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_file_matches_sha256_hex_for_small_and_empty_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let small = dir.path().join("small.bin");
    std::fs::write(&small, b"abc").expect("write small");
    assert_eq!(sha256_file(&small).expect("hash small"), sha256_hex(b"abc"));

    let empty = dir.path().join("empty.bin");
    std::fs::write(&empty, b"").expect("write empty");
    assert_eq!(sha256_file(&empty).expect("hash empty"), sha256_hex(b""));
}

#[test]
fn sha256_file_matches_sha256_hex_across_many_chunks() {
    // Well past one 64 KiB read and not a multiple of it, so the streaming
    // loop reads several full chunks and then a partial tail.
    let contents: Vec<u8> = b"the quick brown fox jumps over the lazy dog\n"
        .iter()
        .copied()
        .cycle()
        .take(300_003)
        .collect();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("multi-chunk.bin");
    std::fs::write(&path, &contents).expect("write multi-chunk");
    assert_eq!(
        sha256_file(&path).expect("hash multi-chunk"),
        sha256_hex(&contents)
    );
}

#[test]
fn sha256_file_reports_a_missing_file_as_io_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("absent.bin");
    let err = sha256_file(&missing).expect_err("a missing file cannot be hashed");
    assert_eq!(err.code(), "io_error");
    let Error::Io {
        action,
        path,
        source,
    } = err
    else {
        panic!("expected Error::Io, got {err:?}");
    };
    assert_eq!(action, "read for hashing");
    assert_eq!(path, missing);
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
}
