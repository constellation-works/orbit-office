//! Document capture contract through the built exec binary, with temporary roots.

use super::{Env, git};
use serde_json::{Value, json};

fn initialized() -> Env {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    env
}

fn upload(env: &Env, name: &str, bytes: &[u8]) -> Value {
    let path = env.workspace.join(name);
    std::fs::write(&path, bytes).unwrap();
    env.tool("office.capture", json!({"upload_path": path}))
}

fn show(env: &Env, captured: &Value, anchor: Option<&str>) -> Value {
    assert_eq!(captured["ok"], true, "{captured}");
    env.tool(
        "office.source-show",
        json!({"id": captured["output"]["id"], "anchor": anchor}),
    )
}

#[test]
fn html_and_docx_preserve_structure_and_resolve_line_anchors() {
    let env = initialized();
    let html = upload(
        &env,
        "confluence.html",
        include_bytes!("../fixtures/capture/confluence.html"),
    );
    let shown = show(&env, &html, None);
    assert_eq!(html["output"]["kind"], "doc");
    let body = shown["output"]["body"].as_str().unwrap();
    for expected in [
        "# Onboarding & operations",
        "- First step",
        "- Second step",
        "| Owner | Team |",
        "| Alice | Platform |",
    ] {
        assert!(body.contains(expected), "missing {expected}: {shown}");
    }
    for removed in [
        "secretScript",
        "hidden",
        "Navigation secret",
        "Metadata navigation",
        "bad",
    ] {
        assert!(!body.contains(removed), "retained {removed}: {shown}");
    }
    assert_eq!(
        show(&env, &html, Some("#L1"))["output"]["body"],
        "# Onboarding & operations"
    );

    let docx = upload(
        &env,
        "guide.docx",
        include_bytes!("../fixtures/capture/guide.docx"),
    );
    let shown = show(&env, &docx, None);
    let body = shown["output"]["body"].as_str().unwrap();
    assert!(body.contains("# Operations guide"), "{shown}");
    assert!(body.contains("- First action & follow-up"), "{shown}");
    assert_eq!(
        show(&env, &docx, Some("#L1"))["output"]["body"],
        "# Operations guide"
    );

    for (name, bytes) in [
        ("readme.md", b"# Heading\n\n- One\n".as_slice()),
        ("plain.txt", b"Plain text\n".as_slice()),
    ] {
        let result = upload(&env, name, bytes);
        assert_eq!(
            show(&env, &result, None)["output"]["body"],
            std::str::from_utf8(bytes).unwrap().trim_end()
        );
    }
}

#[test]
fn pptx_uses_presentation_order_with_slide_body_and_related_notes() {
    let env = initialized();
    let captured = upload(
        &env,
        "brief.pptx",
        include_bytes!("../fixtures/capture/brief.pptx"),
    );
    let first = show(&env, &captured, Some("#slide=1"));
    let body = first["output"]["body"].as_str().unwrap();
    for expected in [
        "Opening title",
        "- First slide action",
        "Speaker notes for opening.",
    ] {
        assert!(body.contains(expected), "{first}");
    }
    assert!(!body.contains("Closing title"), "{first}");
    let second = show(&env, &captured, Some("#slide=2"));
    assert!(
        second["output"]["body"]
            .as_str()
            .unwrap()
            .contains("Closing title")
    );
    for (anchor, code) in [
        ("#slide=0", "anchor_invalid"),
        ("#slide=3", "anchor_out_of_range"),
        ("#page=1", "anchor_out_of_range"),
    ] {
        assert_eq!(show(&env, &captured, Some(anchor))["error"]["code"], code);
    }
    let empty = upload(
        &env,
        "empty.pptx",
        include_bytes!("../fixtures/capture/empty.pptx"),
    );
    assert!(
        show(&env, &empty, None)["output"]["body"]
            .as_str()
            .unwrap()
            .contains("needs_ocr")
    );
}

/// A deterministic converter protocol fixture; no dependency on host poppler.
/// Check flags, environment scrubbing, and that the input is the pinned PDF.
#[cfg(unix)]
fn converter(env: &Env, output: &str, exit: u8) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin = env.workspace.join("converter-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join("pdftotext");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
[ "$1" = -layout ] && [ "$2" = -enc ] && [ "$3" = UTF-8 ] && [ "$5" = - ] || exit 11
[ -z "$OFFICE_TEST_SECRET" ] && [ -z "$ORBIT_TEST_SECRET" ] || exit 12
IFS= read -r header < "$4"
[ "$header" = %PDF-1.4 ] || exit 13
printf '%b' '{output}'
exit {exit}
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

#[cfg(unix)]
#[test]
fn pdf_pages_ocr_and_converter_refusals_are_atomic() {
    let env = initialized();
    let input = env.workspace.join("guide.pdf");
    std::fs::write(&input, include_bytes!("../fixtures/capture/guide.pdf")).unwrap();
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let empty_path = env.workspace.join("empty-bin");
    std::fs::create_dir(&empty_path).unwrap();
    // Store construction needs git even when the document converter is absent.
    let git_binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    std::os::unix::fs::symlink(git_binary, empty_path.join("git")).unwrap();
    let source_entries = std::fs::read_dir(env.library().join("sources"))
        .unwrap()
        .count();
    let refused = env.tool_with_path(
        "office.capture",
        json!({"upload_path": input}),
        empty_path.to_str().unwrap(),
    );
    assert_eq!(
        refused["error"]["code"], "converter_unavailable",
        "{refused}"
    );
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("pdftotext")
    );
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_dir(env.library().join("sources"))
            .unwrap()
            .count(),
        source_entries
    );

    let path = converter(&env, "Page one text\\fPage two text\\f", 0);
    let captured = env.tool_with_path("office.capture", json!({"upload_path": input}), &path);
    let first = show(&env, &captured, Some("#page=1"));
    assert_eq!(first["output"]["body"], "# Page 1\n\nPage one text");
    assert_eq!(
        show(&env, &captured, Some("#page=2"))["output"]["body"],
        "# Page 2\n\nPage two text"
    );
    assert_eq!(
        show(&env, &captured, Some("#page=3"))["error"]["code"],
        "anchor_out_of_range"
    );

    let scanned = env.workspace.join("scanned.pdf");
    std::fs::write(&scanned, include_bytes!("../fixtures/capture/scanned.pdf")).unwrap();
    let path = converter(&env, "\\f", 0);
    let captured = env.tool_with_path("office.capture", json!({"upload_path": scanned}), &path);
    let shown = show(&env, &captured, None);
    assert!(
        shown["output"]["body"]
            .as_str()
            .unwrap()
            .contains("needs_ocr"),
        "{shown}"
    );

    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let path = converter(&env, "secret diagnostics", 2);
    let refused = env.tool_with_path("office.capture", json!({"upload_path": scanned}), &path);
    assert_eq!(refused["error"]["code"], "conversion_failed", "{refused}");
    assert!(!refused.to_string().contains("secret diagnostics"));
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
}

#[test]
fn malformed_or_unsafe_office_archives_leave_no_sources_or_commits() {
    let env = initialized();
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let source_entries = std::fs::read_dir(env.library().join("sources"))
        .unwrap()
        .count();
    for (name, bytes) in [
        ("bad.docx", b"not a zip".as_slice()),
        ("bad.pptx", b"not a zip".as_slice()),
        (
            "missing.docx",
            include_bytes!("../fixtures/capture/missing.docx").as_slice(),
        ),
        (
            "missing.pptx",
            include_bytes!("../fixtures/capture/missing.pptx").as_slice(),
        ),
        (
            "missing-slide.pptx",
            include_bytes!("../fixtures/capture/missing-slide.pptx").as_slice(),
        ),
        (
            "missing-notes.pptx",
            include_bytes!("../fixtures/capture/missing-notes.pptx").as_slice(),
        ),
        (
            "malformed.docx",
            include_bytes!("../fixtures/capture/malformed.docx").as_slice(),
        ),
        (
            "escape.docx",
            include_bytes!("../fixtures/capture/escape.docx").as_slice(),
        ),
    ] {
        let captured = upload(&env, name, bytes);
        assert_eq!(
            captured["error"]["code"], "document_invalid",
            "{name}: {captured}"
        );
        assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
        assert_eq!(
            std::fs::read_dir(env.library().join("sources"))
                .unwrap()
                .count(),
            source_entries
        );
    }
    assert!(!env.workspace.join("outside.txt").exists());
    assert!(!env._dir.path().join("outside.txt").exists());
}

#[test]
fn capture_tags_round_trip_into_search_and_reindex_without_metadata_injection() {
    let env = initialized();
    let captured = env.tool("office.capture", json!({"text": "Bootstrap handbook", "tags": ["bootstrap:orientation", "quote\"tag", "comma,tag"]}));
    assert_eq!(captured["ok"], true, "{captured}");
    for tag in ["quote\"tag", "comma,tag"] {
        let found = env.tool("office.search", json!({"query": "handbook", "tag": tag}));
        assert_eq!(found["output"]["count"], 1, "{found}");
    }
    for reindex in [false, true] {
        if reindex {
            assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
        }
        let found = env.tool(
            "office.search",
            json!({"query": "handbook", "tag": "bootstrap:orientation"}),
        );
        assert_eq!(found["output"]["count"], 1, "{found}");
        assert_eq!(found["output"]["hits"][0]["id"], captured["output"]["id"]);
        let excluded = env.tool(
            "office.search",
            json!({"query": "handbook", "tag": "bootstrap:other"}),
        );
        assert_eq!(excluded["output"]["count"], 0, "{excluded}");
    }
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    for tags in [
        json!("invalid"),
        json!([42]),
        json!(["bad\nsensitivity: normal"]),
    ] {
        let refused = env.tool(
            "office.capture",
            json!({"text": "Must not persist", "tags": tags}),
        );
        assert_eq!(refused["error"]["code"], "invalid_argument", "{refused}");
        assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    }
    let duplicate = env.tool(
        "office.capture",
        json!({"text": "Bootstrap handbook", "tags": ["bootstrap:next"]}),
    );
    assert_eq!(duplicate["output"]["id"], captured["output"]["id"]);
    let found = env.tool(
        "office.search",
        json!({"query": "handbook", "tag": "bootstrap:next"}),
    );
    assert_eq!(found["output"]["count"], 1, "{found}");
    let conversation = env.tool("office.capture", json!({"kind": "conversation", "title": "Onboarding session", "tags": ["bootstrap:first"], "entries": [{"kind": "fact", "text": "Initial handbook."}]}));
    let before = show(&env, &conversation, None);
    let appended = env.tool("office.capture", json!({"append_to": conversation["output"]["id"], "tags": ["bootstrap:second"], "entries": [{"kind": "handoff", "text": "Continue handbook."}]}));
    let after = show(&env, &appended, None);
    assert!(
        after["output"]["body"]
            .as_str()
            .unwrap()
            .starts_with(before["output"]["body"].as_str().unwrap())
    );
    for tag in ["bootstrap:first", "bootstrap:second"] {
        let found = env.tool("office.search", json!({"query": "handbook", "tag": tag}));
        assert_eq!(found["output"]["count"], 1, "{found}");
    }
}
