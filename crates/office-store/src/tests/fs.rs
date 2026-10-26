//! Filesystem mechanics: path safety and atomic writes.

use crate::fs::atomic::write_atomic;
use crate::fs::hash::sha256_hex;
use crate::fs::path_safety::validate_path_stem;

#[test]
fn path_stems_accept_slugs_and_refuse_traversal() {
    assert!(validate_path_stem("src-20261028-acme-qbr", "source").is_ok());
    assert!(validate_path_stem("../escape", "source").is_err());
    assert!(validate_path_stem("a/b", "source").is_err());
    assert!(validate_path_stem(".hidden", "source").is_err());
    assert!(validate_path_stem("", "source").is_err());
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
