use super::library_support::{Env, git};

#[test]
fn source_sensitivity_is_monotonic_through_the_binary() {
    let env = Env::with_initialized_library();
    let captured = env.tool(
        "office.capture",
        serde_json::json!({
            "kind":"doc", "title":"Privacy", "text":"Preserve this body.", "sensitivity":"normal"
        }),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let id = &captured["output"]["id"];
    let path = env
        .library()
        .join(captured["output"]["path"].as_str().unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    let upgraded = env.tool(
        "office.source-set-sensitivity",
        serde_json::json!({"id":id,"sensitivity":"private"}),
    );
    assert_eq!(upgraded["ok"], true, "{upgraded}");
    assert_eq!(upgraded["output"]["sensitivity"], "private");
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after,
        before.replace("sensitivity: normal", "sensitivity: private")
    );
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let same = env.tool(
        "office.source-set-sensitivity",
        serde_json::json!({"id":id,"sensitivity":"private"}),
    );
    assert_eq!(same["ok"], true, "{same}");
    let refused = env.tool(
        "office.source-set-sensitivity",
        serde_json::json!({"id":id,"sensitivity":"normal"}),
    );
    assert_eq!(
        refused["error"]["code"], "sensitivity_downgrade_refused",
        "{refused}"
    );
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("sensitivity")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), after);
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    let shown = env.tool("office.source-show", serde_json::json!({"id":id}));
    assert_eq!(shown["output"]["sensitivity"], "private", "{shown}");
    // Private bodies are withheld from tool callers (see
    // library_reads::private_bodies_and_snippets_are_withheld_through_the_binary).
    assert_eq!(shown["output"]["content_withheld"], true, "{shown}");
    assert_eq!(shown["output"]["body"], "");
    assert!(
        !shown.to_string().contains("Preserve this body."),
        "{shown}"
    );
    let store = office_store::LibraryStore::open(&env.library(), &env.state).unwrap();
    assert_eq!(
        store
            .get(id.as_str().unwrap())
            .unwrap()
            .unwrap()
            .sensitivity,
        "private"
    );
}

#[test]
fn capture_and_source_show_through_the_binary() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");

    // Pasted text dedupes to one id.
    let first = env.tool(
        "office.capture",
        serde_json::json!({"kind": "doc", "title": "Binary note", "text": "Captured through the binary."}),
    );
    assert_eq!(first["ok"], serde_json::json!(true), "{first}");
    let second = env.tool(
        "office.capture",
        serde_json::json!({"kind": "doc", "title": "Binary note", "text": "Captured through the binary."}),
    );
    assert_eq!(second["output"]["id"], first["output"]["id"], "{second}");
    assert_eq!(second["output"]["deduplicated"], serde_json::json!(true));

    // A video upload with a VTT transcript needs no model call.
    let video = env.workspace.join("talk.mp4");
    std::fs::write(&video, b"video-bytes").expect("video");
    let vtt = env.workspace.join("talk.vtt");
    std::fs::write(
        &vtt,
        "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\n<v Jane>Opening line.</v>\n",
    )
    .expect("vtt");
    let captured = env.tool(
        "office.capture",
        serde_json::json!({
            "upload_path": video.to_string_lossy(),
            "transcript_path": vtt.to_string_lossy(),
            "title": "Binary KT",
        }),
    );
    assert_eq!(
        captured["output"]["kind"],
        serde_json::json!("video"),
        "{captured}"
    );
    let shown = env.tool(
        "office.source-show",
        serde_json::json!({"id": captured["output"]["id"], "anchor": "#t=00:00:01"}),
    );
    assert!(
        shown["output"]["body"]
            .as_str()
            .expect("body")
            .contains("Jane"),
        "{shown}"
    );
    let bad_anchor = env.tool(
        "office.source-show",
        serde_json::json!({"id": captured["output"]["id"], "anchor": "#L42"}),
    );
    assert_eq!(
        bad_anchor["error"]["code"],
        serde_json::json!("anchor_out_of_range"),
        "{bad_anchor}"
    );

    // A symlink escaping the workspace is refused.
    let outside = env._dir.path().join("outside.bin");
    std::fs::write(&outside, b"outside").expect("outside");
    let link = env.workspace.join("escape.bin");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    let refused = env.tool(
        "office.capture",
        serde_json::json!({"upload_path": link.to_string_lossy(), "title": "Escape"}),
    );
    assert_eq!(
        refused["error"]["code"],
        serde_json::json!("upload_outside_allowed_roots"),
        "{refused}"
    );

    // Conversation appends preserve earlier lines.
    let opened = env.tool(
        "office.capture",
        serde_json::json!({"kind": "conversation", "title": "Binary session", "entries": [{"kind": "fact", "text": "Fact one."}]}),
    );
    let id = opened["output"]["id"].as_str().expect("id").to_string();
    let appended = env.tool(
        "office.capture",
        serde_json::json!({"append_to": id, "entries": [{"kind": "handoff", "text": "Next step."}]}),
    );
    assert_eq!(
        appended["output"]["appended"],
        serde_json::json!(1),
        "{appended}"
    );
    let log = env.tool("office.source-show", serde_json::json!({"id": id}));
    let body = log["output"]["body"].as_str().expect("body").to_string();
    assert!(body.contains("fact: Fact one."), "{body}");
    assert!(body.contains("handoff: Next step."), "{body}");
}

#[test]
fn source_show_and_knowledge_write_refuse_timestamp_overflow() {
    let env = Env::with_initialized_library();
    let captured = env.tool(
        "office.capture",
        serde_json::json!({"kind": "meeting", "title": "Zero", "text": "[00:00:00] Jane: zero"}),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let id = captured["output"]["id"].as_str().unwrap();
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    for (stamp, code) in [
        ("1152921504606846976:00:00", "anchor_invalid"),
        ("5124095576030431:00:16", "anchor_invalid"),
        ("5124095576040:25:51", "anchor_out_of_range"),
    ] {
        for markdown in [
            format!(
                "---\nid: dom-overflow\ntitle: Overflow\ncites: [\"{id}#t={stamp}\"]\n---\nClaim.\n"
            ),
            format!("---\nid: dom-overflow\ntitle: Overflow\n---\nClaim [{id}#t={stamp}].\n"),
        ] {
            let written = env.tool(
                "office.knowledge-write",
                serde_json::json!({"markdown": markdown}),
            );
            assert_eq!(written["ok"], false, "{written}");
            assert_eq!(written["error"]["code"], "unresolved_cite", "{written}");
            assert!(
                !env.library()
                    .join("knowledge/domain/dom-overflow.md")
                    .exists()
            );
        }
        let shown = env.tool(
            "office.source-show",
            serde_json::json!({"id": id, "anchor": format!("#t={stamp}")}),
        );
        assert_eq!(shown["ok"], false, "{shown}");
        assert_eq!(shown["error"]["code"], code, "{shown}");
    }
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
}

#[test]
fn vtt_capture_refuses_timestamp_overflow_before_persisting() {
    let env = Env::with_initialized_library();
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let transcript = env.workspace.join("overflow.vtt");
    for stamp in [
        "1152921504606846976:00:00.000",
        "5124095576030431:00:16.000",
        "5124095576030:25:52.000",
        "5124095576030:25:51.616",
        "18446744073709551615:00.000",
    ] {
        for (start, end) in [(stamp, "00:00:03.000"), ("00:00:01.000", stamp)] {
            std::fs::write(
                &transcript,
                format!("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nJane: valid first cue\n\n{start} --> {end}\nJane: overflow\n"),
            ).unwrap();
            let input = serde_json::json!({"kind": "meeting", "title": "Overflow", "text": "fallback", "transcript_path": transcript});
            let response = env.tool("office.capture", input);
            assert_eq!(response["ok"], false, "{stamp}: {response}");
            assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
            assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
            assert!(git(&env.library(), &["status", "--porcelain"]).is_empty());
        }
    }
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["output"]["library"]["documents"], 0, "{status}");
}

#[test]
fn capture_show_and_cite_preserve_supported_timestamp_boundaries() {
    let env = Env::with_initialized_library();
    let transcript = env.workspace.join("long.vtt");
    for (index, stamp) in ["100:00:00", "5124095576030:25:51"].iter().enumerate() {
        std::fs::write(
            &transcript,
            format!("WEBVTT\n\n{stamp}.615 --> {stamp}.615 align:start\nJane: valid\n"),
        )
        .unwrap();
        let captured = env.tool(
            "office.capture",
            serde_json::json!({"kind": "meeting", "title": "Long", "text": "fallback", "transcript_path": transcript}),
        );
        assert_eq!(captured["ok"], true, "{captured}");
        let id = captured["output"]["id"].as_str().unwrap();
        let shown = env.tool(
            "office.source-show",
            serde_json::json!({"id": id, "anchor": format!("#t={stamp}")}),
        );
        assert_eq!(shown["ok"], true, "{shown}");
        assert_eq!(shown["output"]["body"], format!("[{stamp}] Jane: valid"));
        let written = env.tool(
            "office.knowledge-write",
            serde_json::json!({"markdown": format!("---\nid: dom-long-{index}\ntitle: Long\ncites: [\"{id}#t={stamp}\"]\n---\nClaim [{id}#t={stamp}].\n")}),
        );
        assert_eq!(written["ok"], true, "{written}");
    }
}

#[test]
fn private_capture_metadata_cannot_inject_frontmatter() {
    for (field, payload) in [
        ("captured_by", "paste\nsensitivity: normal\n---\ninjected"),
        (
            "occurred_at",
            "2026-10-27\nsensitivity: normal\n---\ninjected",
        ),
        ("participants", "Alice]\nsensitivity: normal\n---\ninjected"),
    ] {
        let env = Env::with_initialized_library();
        let mut input = serde_json::json!({"kind": "doc", "title": "Private fixture", "text": "original private body\n", "sensitivity": "private"});
        input[field] = if field == "participants" {
            serde_json::json!([payload])
        } else {
            serde_json::json!(payload)
        };
        let captured = env.tool("office.capture", input);
        assert_eq!(captured["ok"], true, "{field}: {captured}");
        assert_eq!(captured["output"]["sensitivity"], "private");
        let id = captured["output"]["id"].as_str().unwrap();
        for reindex in [false, true] {
            if reindex {
                let result = env.tool("office.reindex", serde_json::json!({}));
                assert_eq!(result["ok"], true, "{result}");
            }
            let shown = env.tool("office.source-show", serde_json::json!({"id": id}));
            assert_eq!(
                shown["output"]["sensitivity"], "private",
                "{field}: {shown}"
            );
            // The stored body is intact (indexed.body below); tool callers
            // get it withheld because the source is private.
            assert_eq!(shown["output"]["body"], "", "{field}: {shown}");
            assert_eq!(
                shown["output"]["content_withheld"], true,
                "{field}: {shown}"
            );
            let store = office_store::LibraryStore::open(&env.library(), &env.state).unwrap();
            let indexed = store.get(id).unwrap().unwrap();
            assert_eq!(indexed.sensitivity, "private", "{field}: {indexed:?}");
            assert_eq!(
                indexed.body, "original private body",
                "{field}: {indexed:?}"
            );
        }
    }
}

#[test]
fn private_upload_extension_cannot_inject_frontmatter() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], true, "{init}");

    let upload = env.workspace.join("private.txt\nsensitivity: normal");
    std::fs::write(&upload, "private uploaded body\n").expect("upload");
    let captured = env.tool(
        "office.capture",
        serde_json::json!({
            "upload_path": upload.to_string_lossy(),
            "title": "Private upload",
            "sensitivity": "private"
        }),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    assert_eq!(captured["output"]["sensitivity"], "private", "{captured}");
    let id = captured["output"]["id"].as_str().expect("source id");

    let shown = env.tool("office.source-show", serde_json::json!({"id": id}));
    assert_eq!(shown["ok"], true, "{shown}");
    assert_eq!(shown["output"]["sensitivity"], "private", "{shown}");
    // The stored body is intact (indexed.body below); tool callers get it
    // withheld because the source is private.
    assert_eq!(shown["output"]["body"], "", "{shown}");
    assert_eq!(shown["output"]["content_withheld"], true, "{shown}");

    let source_path = captured["output"]["path"].as_str().expect("source path");
    let original_path = env
        .library()
        .join(source_path)
        .parent()
        .expect("source directory")
        .join("original.txt");
    assert_eq!(
        std::fs::read(original_path).expect("stored original"),
        b"private uploaded body\n"
    );

    let store = office_store::LibraryStore::open(&env.library(), &env.state).expect("store");
    let indexed = store.get(id).expect("indexed source").expect("source");
    assert_eq!(indexed.sensitivity, "private", "{indexed:?}");
    assert_eq!(indexed.body, "private uploaded body", "{indexed:?}");
}
