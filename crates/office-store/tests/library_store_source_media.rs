//! Source-media access scenarios for the `library_store` integration tests.
//! Split from `library_store.rs`; scenario names and assertions are unchanged.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "library_store_support.rs"]
mod support;

use support::*;

use office_store::LibraryStore;
use std::path::Path;

#[test]
fn bounded_source_media_reads_disk_frames_and_refuses_active_writers_and_tampering() {
    use office_core::domain::source::{
        access::{MEDIA_LIMIT, MediaSelection},
        sha256_hex,
    };
    use office_core::ports::Library;
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    let store = LibraryStore::open(&library, &state).unwrap();
    let source = "sources/1970/01/src-media/source.md";
    let markdown = format!(
        "---\nid: src-media\nkind: video\ntitle: Media\nsensitivity: normal\noriginal: {{ file: original.mp4, bytes: 5, sha256: {}, media_type: video/mp4 }}\n---\nBody.\n",
        sha256_hex(b"video")
    );
    store.write_document(Path::new(source), &markdown).unwrap();
    let original = library.join(Path::new(source).with_file_name("original.mp4"));
    std::fs::write(&original, b"video").unwrap();
    let frame = Path::new(source).with_file_name("frames/0000.png");
    std::fs::create_dir_all(library.join(&frame).parent().unwrap()).unwrap();
    std::fs::write(library.join(&frame), b"png").unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .unwrap()
            .content
            .unwrap(),
        b"video"
    );
    assert_eq!(
        Library::source_media(
            &store,
            "src-media",
            &MediaSelection::Frame("0000.png".into())
        )
        .unwrap()
        .content
        .unwrap(),
        b"png"
    );
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("library.writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let error = Library::source_media(&store, "src-media", &MediaSelection::Original)
        .err()
        .unwrap();
    assert_eq!(error.code(), "library_writer_busy");
    drop(lock);
    std::fs::write(&original, b"changed").unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .err()
            .unwrap()
            .code(),
        "plan_changed"
    );
    for size in [MEDIA_LIMIT, MEDIA_LIMIT + 1] {
        std::fs::write(library.join(&frame), vec![255; size]).unwrap();
        let media = Library::source_media(
            &store,
            "src-media",
            &MediaSelection::Frame("0000.png".into()),
        )
        .unwrap();
        assert_eq!(media.bytes, u64::try_from(size).unwrap());
        assert_eq!(media.content.is_none(), size > MEDIA_LIMIT);
    }
    std::fs::write(&original, vec![0; MEDIA_LIMIT + 1]).unwrap();
    assert!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .unwrap()
            .content
            .is_none()
    );
    std::fs::remove_file(&original).unwrap();
    std::fs::create_dir(&original).unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .err()
            .unwrap()
            .code(),
        "source_media_unavailable"
    );
    // An upgrade is checked against the latest file before a media payload read.
    Library::set_source_sensitivity(&store, "src-media", "private").unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .err()
            .unwrap()
            .code(),
        "private_endpoint_unconfigured"
    );
}

#[cfg(unix)]
#[test]
fn source_media_refuses_symlink_components_and_original_traversal() {
    use office_core::domain::source::{access::MediaSelection, sha256_hex};
    use office_core::ports::Library;
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    initialize(&library);
    let store = LibraryStore::open(&library, &dir.path().join("state")).unwrap();
    let source = Path::new("sources/1970/01/src-media/source.md");
    let markdown = format!(
        "---\nid: src-media\nkind: video\ntitle: Media\nsensitivity: normal\noriginal: {{ file: original.mp4, bytes: 5, sha256: {}, media_type: video/mp4 }}\n---\nBody.\n",
        sha256_hex(b"video")
    );
    store.write_document(source, &markdown).unwrap();
    let outside = dir.path().join("outside.mp4");
    std::fs::write(&outside, b"video").unwrap();
    std::os::unix::fs::symlink(
        &outside,
        library.join(source.with_file_name("original.mp4")),
    )
    .unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .err()
            .unwrap()
            .code(),
        "unsafe_path"
    );
    let outside_frames = dir.path().join("frames");
    std::fs::create_dir(&outside_frames).unwrap();
    std::fs::write(outside_frames.join("0000.png"), b"png").unwrap();
    std::os::unix::fs::symlink(
        outside_frames,
        library.join(source.with_file_name("frames")),
    )
    .unwrap();
    assert_eq!(
        Library::source_media(
            &store,
            "src-media",
            &MediaSelection::Frame("0000.png".into())
        )
        .err()
        .unwrap()
        .code(),
        "unsafe_path"
    );
    store
        .write_document(
            source,
            &markdown.replace("file: original.mp4", "file: original../outside.mp4"),
        )
        .unwrap();
    assert_eq!(
        Library::source_media(&store, "src-media", &MediaSelection::Original)
            .err()
            .unwrap()
            .code(),
        "unsafe_path"
    );
}
