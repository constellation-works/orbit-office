//! Filesystem mechanics: path safety and atomic writes.

use std::path::Path;

use crate::Error;
use crate::fs::atomic::{write_atomic, write_atomic_new};
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
fn atomic_create_publishes_complete_bytes_and_preserves_existing_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("original.bin");
    write_atomic_new(&path, &[0, 255, 128]).expect("publish original");
    for bytes in [&[0, 255, 128][..], b"replacement"] {
        let error = write_atomic_new(&path, bytes).expect_err("never replace even identical bytes");
        assert!(matches!(error, Error::Io { source, .. }
            if source.kind() == std::io::ErrorKind::AlreadyExists));
        assert_eq!(
            std::fs::read(&path).expect("original preserved"),
            [0, 255, 128]
        );
    }
    let directory = dir.path().join("directory");
    std::fs::create_dir(&directory).expect("occupied directory");
    assert!(write_atomic_new(&directory, b"replacement").is_err());
    assert!(directory.is_dir());
    assert_eq!(
        std::fs::read_dir(dir.path())
            .expect("no leaked temps")
            .count(),
        2
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn atomic_create_preserves_a_dangling_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let victim = dir.path().join("missing");
    let link = dir.path().join("original.bin");
    std::os::unix::fs::symlink(&victim, &link).expect("planted link");
    let error = write_atomic_new(&link, b"replacement").expect_err("refuse link");
    assert!(matches!(error, Error::Io { source, .. }
        if source.kind() == std::io::ErrorKind::AlreadyExists));
    assert_eq!(std::fs::read_link(&link).unwrap(), victim);
    assert!(!victim.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn atomic_write_survives_concurrent_writers_to_the_same_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("shared.md");
    const THREADS: usize = 8;
    const WRITES_PER_THREAD: usize = 25;
    let payloads: Vec<Vec<u8>> = (0..THREADS * WRITES_PER_THREAD)
        .map(|n| format!("concurrent-payload-{n:04}").into_bytes())
        .collect();
    std::thread::scope(|scope| {
        for chunk in payloads.chunks(WRITES_PER_THREAD) {
            let target = &path;
            scope.spawn(move || {
                for payload in chunk {
                    write_atomic(target, payload).expect("concurrent write");
                }
            });
        }
    });
    let final_contents = std::fs::read(&path).expect("read back");
    assert!(
        payloads.contains(&final_contents),
        "final file must be one complete payload, got {final_contents:?}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

#[test]
fn atomic_write_never_follows_a_planted_symlink_at_the_legacy_temp_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("page.md");
    // The temp name the pre-fix code always used: process id plus target
    // name, with no per-call uniqueness.
    let legacy_tmp = dir
        .path()
        .join(format!(".office-store-{}-page.md.tmp", std::process::id()));
    let victim = dir.path().join("victim.md");
    std::fs::write(&victim, b"victim-contents").expect("write victim");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&victim, &legacy_tmp).expect("plant symlink");
    #[cfg(not(unix))]
    std::fs::write(&legacy_tmp, b"planted-blocker").expect("plant blocker");
    write_atomic(&path, b"payload").expect("write around planted entry");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        "payload"
    );
    #[cfg(unix)]
    {
        assert_eq!(
            std::fs::read_to_string(&victim).expect("victim intact"),
            "victim-contents"
        );
        assert!(
            std::fs::symlink_metadata(&legacy_tmp)
                .expect("planted entry still present")
                .file_type()
                .is_symlink(),
            "planted symlink must not be replaced or removed"
        );
    }
    #[cfg(not(unix))]
    assert_eq!(
        std::fs::read_to_string(&legacy_tmp).expect("planted entry intact"),
        "planted-blocker"
    );
}

#[test]
#[cfg(unix)]
fn atomic_write_leaves_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("secret.md");
    write_atomic(&path, b"payload").expect("write");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "final mode must be owner-only, got {mode:o}");
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
