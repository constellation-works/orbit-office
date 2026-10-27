//! `office.source-show` through dispatch against the fake Library (STD-04).

use office_core::ports::Library;
use serde_json::{Value, json};
use std::path::Path;

const ID: &str = "src-19700101-source-view";
const SOURCE_PATH: &str = "sources/1970/01/src-19700101-source-view/source.md";

fn fixture(
    bytes: &[u8],
    private: bool,
    settings: office_core::domain::settings::Settings,
) -> office_testkit::fakes::FakeLibrary {
    use office_core::domain::source::sha256_hex;
    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.settings = settings;
    let markdown = format!(
        "---\nid: {ID}\nkind: video\ntitle: Fixture\nsensitivity: {}\noriginal: {{ file: original.mp4, sha256: {}, bytes: {}, media_type: video/mp4 }}\nderived_by: {{ transcription: vtt-import }}\n---\nTranscript.\n",
        if private { "private" } else { "normal" },
        sha256_hex(bytes),
        bytes.len()
    );
    library
        .insert_video(Path::new(SOURCE_PATH), &markdown, bytes)
        .unwrap();
    library.write_source_processing(ID, &sha256_hex(markdown.as_bytes()), None,
        &office_core::ports::DerivedBy { route:office_core::domain::endpoint::ModelRoute::Hosted, model:"fixture".into() },
        &[("chapters.md".into(), b"## Chapter 1: Start [core]\n\n[00:00:00](#t=00:00:00)\n\n## Chapter 2: Aside [tangent]\n\n[00:01:05](#t=00:01:05)\n".to_vec())]).unwrap();
    for (path, text) in [
        (
            "knowledge/domain/dom-page.md",
            format!("---\nid: dom-page\ntitle: Page\ncites: [{ID}#L1]\n---\nPage."),
        ),
        (
            "artifacts/art-output/artifact.md",
            format!("---\nid: art-output\ntitle: Output\ncites: [{ID}]\n---\nOutput."),
        ),
    ] {
        library.write_document(Path::new(path), &text).unwrap();
    }
    library.insert_source_frame(ID, "0000.png", b"png").unwrap();
    library
}

fn ports(library: office_testkit::fakes::FakeLibrary) -> office_core::ports::Ports<'static> {
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports
}

fn call(tool: &str, input: Value, ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

#[test]
fn source_show_returns_line_anchors_and_refuses_bad_anchors() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({"kind": "chat", "title": "Thread", "text": "one\ntwo\nthree\nfour"}),
        &ports,
    );
    let id = captured["output"]["id"].as_str().expect("id").to_string();
    let span = call(
        "office.source-show",
        json!({"id": id, "anchor": "#L2-3"}),
        &ports,
    );
    assert_eq!(span["output"]["body"], json!("two\nthree"), "{span}");
    let malformed = call(
        "office.source-show",
        json!({"id": id, "anchor": "banana"}),
        &ports,
    );
    assert_eq!(malformed["error"]["code"], json!("anchor_invalid"));
    let out_of_range = call(
        "office.source-show",
        json!({"id": id, "anchor": "#L99"}),
        &ports,
    );
    assert_eq!(out_of_range["error"]["code"], json!("anchor_out_of_range"));
    assert_ne!(
        malformed["error"]["code"], out_of_range["error"]["code"],
        "malformed and out-of-range anchors have distinct codes"
    );
    let missing = call(
        "office.source-show",
        json!({"id": "src-00000000-nope"}),
        &ports,
    );
    assert_eq!(missing["error"]["code"], json!("source_not_found"));
}

#[test]
fn source_show_refuses_timestamp_overflow_through_dispatch() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({"kind": "meeting", "title": "Zero", "text": "[00:00:00] Jane: zero"}),
        &ports,
    );
    assert_eq!(captured["ok"], json!(true), "{captured}");
    for (stamp, code) in [
        ("1152921504606846976:00:00", "anchor_invalid"),
        ("5124095576030431:00:16", "anchor_invalid"),
        ("5124095576040:25:51", "anchor_out_of_range"),
    ] {
        let response = call(
            "office.source-show",
            json!({"id": captured["output"]["id"], "anchor": format!("#t={stamp}")}),
            &ports,
        );
        assert_eq!(response["ok"], json!(false), "{response}");
        assert_eq!(response["error"]["code"], json!(code), "{response}");
    }
}

#[test]
fn source_show_resolves_interior_time_anchor_to_turn_in_progress() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({
            "kind": "meeting",
            "title": "Meeting",
            "text": "[00:10:00] Jane: one\n\n[00:12:31] Dee: two"
        }),
        &ports,
    );
    let id = captured["output"]["id"].as_str().expect("id").to_string();
    let span = call(
        "office.source-show",
        json!({"id": id, "anchor": "#t=00:11:00"}),
        &ports,
    );
    assert_eq!(span["ok"], json!(true), "{span}");
    assert_eq!(
        span["output"]["body"],
        json!("[00:10:00] Jane: one"),
        "{span}"
    );
}

#[test]
fn source_show_resolves_last_turn_of_single_newline_body() {
    let ports = office_testkit::fakes::PortsBuilder::default().build();
    let captured = call(
        "office.capture",
        json!({
            "kind": "meeting",
            "title": "Meeting",
            "text": "[00:10:00] Jane: one\n[00:12:31] Dee: two"
        }),
        &ports,
    );
    let id = captured["output"]["id"].as_str().expect("id").to_string();
    let span = call(
        "office.source-show",
        json!({"id": id, "anchor": "#t=00:12:31"}),
        &ports,
    );
    assert_eq!(span["ok"], json!(true), "{span}");
    assert_eq!(
        span["output"]["body"],
        json!("[00:12:31] Dee: two"),
        "{span}"
    );
}

#[test]
fn source_show_returns_provenance_pages_artifacts_tasks_and_chapters_without_changing_existing_fields()
 {
    let mut ports = ports(fixture(b"video", false, Default::default()));
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(
        json!({"tasks":[{"id":"fixture-task","title":"Citing task"}],"total":101,"truncated":true}),
    ));
    ports.orbit = Box::new(orbit.clone());
    let response = call(
        "office.source-show",
        json!({"id":ID,"anchor":"#L1"}),
        &ports,
    );
    assert_eq!(response["ok"], true, "{response}");
    let output = &response["output"];
    assert_eq!(output["id"], ID);
    assert_eq!(output["kind"], "video");
    assert_eq!(output["title"], "Fixture");
    assert_eq!(output["path"], SOURCE_PATH);
    assert_eq!(output["body"], "Transcript.");
    assert_eq!(output["text"], output["body"]);
    assert_eq!(output["span"], output["body"]);
    assert_eq!(output["anchor"], "#L1");
    assert_eq!(output["sensitivity"], "normal");
    assert_eq!(
        output["sha256"],
        office_core::domain::source::sha256_hex(b"video")
    );
    assert_eq!(output["bytes"], 5);
    assert_eq!(output["media_type"], "video/mp4");
    assert_eq!(output["version"], 1);
    assert_eq!(
        output["derived_by"],
        json!({"transcription":"vtt-import","extraction":{"route":"hosted","model":"fixture"}})
    );
    assert_eq!(
        output["cited_by"],
        json!([
            {"id":"art-output","title":"Output","kind":"artifact"},
            {"id":"dom-page","title":"Page","kind":"domain"},
            {"id":"fixture-task","title":"Citing task","kind":"task"}
        ])
    );
    assert_eq!(output["task_citers_available"], true);
    assert_eq!(output["cited_by_truncated"], true);
    assert_eq!(
        output["chapters"],
        json!([
            {"number":1,"title":"Start","label":"core","start":0,"anchor":"#t=00:00:00"},
            {"number":2,"title":"Aside","label":"tangent","start":65,"anchor":"#t=00:01:05"}
        ])
    );
    assert!(output.get("media").is_none());
    assert_eq!(orbit.calls().len(), 1);
    assert_eq!(orbit.calls()[0].tool, "orbit.task.list");
    assert_eq!(
        orbit.calls()[0].input,
        json!({"tag":format!("source:{ID}"),"limit":100,"fields":["id","title"]})
            .as_object()
            .unwrap()
            .clone()
    );
}

#[test]
fn source_show_media_is_complete_base64_or_explicitly_over_cap() {
    use office_core::domain::source::access::{MEDIA_LIMIT, base64};
    for bytes in [
        vec![],
        b"v".to_vec(),
        b"vi".to_vec(),
        b"video".to_vec(),
        vec![255; MEDIA_LIMIT],
        vec![0; MEDIA_LIMIT + 1],
    ] {
        let ports = ports(fixture(&bytes, false, Default::default()));
        let response = call(
            "office.source-show",
            json!({"id":ID,"media":"original"}),
            &ports,
        );
        assert_eq!(response["ok"], true, "{response}");
        let media = &response["output"]["media"];
        assert_eq!(media["name"], "original.mp4");
        assert_eq!(media["media_type"], "video/mp4");
        assert_eq!(media["bytes"], bytes.len());
        assert_eq!(media["limit_bytes"], MEDIA_LIMIT);
        assert_eq!(media["too_large"], bytes.len() > MEDIA_LIMIT);
        assert_eq!(media["truncated"], media["too_large"]);
        assert_eq!(
            media["content_base64"],
            if bytes.len() > MEDIA_LIMIT {
                Value::Null
            } else {
                json!(base64(&bytes))
            }
        );
        let frame = call(
            "office.source-show",
            json!({"id":ID,"media":"frame","frame":"0000.png"}),
            &ports,
        );
        assert_eq!(frame["output"]["media"]["name"], "frames/0000.png");
        assert_eq!(frame["output"]["media"]["content_base64"], "cG5n");
    }
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"video"), "dmlkZW8=");
}

#[test]
fn source_media_refuses_invalid_selections_missing_frames_and_private_route_gaps() {
    use office_core::domain::endpoint::{Capability, EndpointKind};
    use office_core::domain::settings::{OpenWeights, Settings};
    let ports = ports(fixture(b"video", false, Default::default()));
    for input in [
        json!({"media":"frame"}),
        json!({"frame":"0000.png"}),
        json!({"media":true}),
        json!({"media":"original","frame":"0000.png"}),
        json!({"media":"frame","frame":"../original.mp4"}),
        json!({"media":"frame","frame":"/0000.png"}),
        json!({"media":"frame","frame":"0000.jpg"}),
    ] {
        let mut input = input.as_object().unwrap().clone();
        input.insert("id".into(), json!(ID));
        let response = call("office.source-show", json!(input), &ports);
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
    let missing = call(
        "office.source-show",
        json!({"id":ID,"media":"frame","frame":"0001.png"}),
        &ports,
    );
    assert_eq!(
        missing["error"]["code"], "source_media_unavailable",
        "{missing}"
    );
    for (caps, verified, code) in [
        (None, false, "private_endpoint_unconfigured"),
        (
            Some(vec![Capability::Text, Capability::Image, Capability::Audio]),
            false,
            "private_endpoint_unverified",
        ),
        (
            Some(vec![Capability::Image, Capability::Audio]),
            true,
            "private_endpoint_unverified",
        ),
        (
            Some(vec![Capability::Text]),
            true,
            "private_media_unsupported",
        ),
        (
            Some(vec![Capability::Text, Capability::Image]),
            true,
            "private_media_unsupported",
        ),
        (
            Some(vec![Capability::Text, Capability::Image, Capability::Audio]),
            true,
            "",
        ),
    ] {
        let settings = Settings {
            open_weights: caps.map(|capabilities| OpenWeights {
                endpoint: "http://127.0.0.1:9999/v1".into(),
                model: "fixture".into(),
                kind: EndpointKind::Company,
                label: "Company".into(),
                capabilities: verified.then_some(capabilities),
                verified_at: verified.then(|| "1970-01-01T00:00:00Z".into()),
            }),
            ..Default::default()
        };
        let ports = self::ports(fixture(b"video", true, settings));
        let response = call(
            "office.source-show",
            json!({"id":ID,"media":"original"}),
            &ports,
        );
        if code.is_empty() {
            assert_eq!(response["ok"], true, "{response}");
        } else {
            assert_eq!(response["error"]["code"], code, "{response}");
        }
        // Private display itself is still available without any media request.
        assert_eq!(
            call("office.source-show", json!({"id":ID}), &ports)["ok"],
            true
        );
    }
}

#[test]
fn source_sensitivity_dispatch_refuses_downgrades_unknown_fields_and_invalid_values() {
    let ports = ports(fixture(b"video", false, Default::default()));
    for input in [
        json!({"id":ID}),
        json!({"id":ID,"sensitivity":null}),
        json!({"id":ID,"sensitivity":"secret"}),
        json!({"id":ID,"sensitivity":"private","override":true}),
    ] {
        assert_eq!(
            call("office.source-set-sensitivity", input, &ports)["error"]["code"],
            "invalid_argument"
        );
    }
    assert_eq!(
        call(
            "office.source-set-sensitivity",
            json!({"id":"src-missing","sensitivity":"private"}),
            &ports
        )["error"]["code"],
        "source_not_found"
    );
    assert_eq!(
        call(
            "office.source-set-sensitivity",
            json!({"id":ID,"sensitivity":"private"}),
            &ports
        )["output"]["sensitivity"],
        "private"
    );
    let refused = call(
        "office.source-set-sensitivity",
        json!({"id":ID,"sensitivity":"normal"}),
        &ports,
    );
    assert_eq!(refused["error"]["code"], "sensitivity_downgrade_refused");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("sensitivity")
    );
    assert_eq!(
        call(
            "office.source-show",
            json!({"id":ID,"media":"original"}),
            &ports
        )["error"]["code"],
        "private_endpoint_unconfigured"
    );
    let tool = crate::tools()
        .iter()
        .find(|t| t.spec().name == "source-set-sensitivity")
        .unwrap();
    assert_eq!(tool.spec().mcp_scope, office_core::tool::McpScope::None);
    assert_eq!(
        tool.spec().execution_kind,
        office_core::tool::ExecutionKind::Mutating
    );
}

#[test]
fn source_read_and_sensitivity_conformance_goldens_replay_through_public_dispatch() {
    for (verb, expected_cases) in [("source-show", 6), ("source-set-sensitivity", 5)] {
        let ports = ports(fixture(b"video", false, Default::default()));
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(path).unwrap();
        let mut input = None;
        let mut cases = 0;
        for line in golden.lines() {
            if let Some(value) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<Value>(value).unwrap());
            }
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(value) = line.strip_prefix(prefix) {
                    let expected: Value = serde_json::from_str(value).unwrap();
                    let response = call(&format!("office.{verb}"), input.take().unwrap(), &ports);
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        assert_eq!(response["output"], expected);
                    } else {
                        assert_eq!(response["error"]["code"], expected["code"]);
                    }
                    cases += 1;
                }
            }
        }
        assert_eq!(
            cases, expected_cases,
            "every success and refusal golden must run"
        );
    }
}

#[test]
fn source_show_marks_unavailable_task_citations_and_refuses_malformed_callback_data() {
    for reply in [
        Err(office_core::Error::OrbitUnavailable),
        Err(office_core::Error::OrbitRefused {
            code: "permission_denied".into(),
        }),
        Ok(json!({"tasks":[],"total":0,"truncated":false})),
        Ok(json!({"tasks":[],"total":1,"truncated":false})),
        Ok(json!({"tasks":[{"id":"fixture-task"}],"total":1,"truncated":false})),
    ] {
        let invalid = reply.as_ref().is_ok_and(|v| v["total"] == 1);
        let available = reply.is_ok();
        let mut ports = ports(fixture(b"video", false, Default::default()));
        let orbit = office_testkit::fakes::FakeOrbit::default();
        orbit.push_response(reply);
        ports.orbit = Box::new(orbit);
        let response = call("office.source-show", json!({"id":ID}), &ports);
        if invalid {
            assert_eq!(
                response["error"]["code"], "source_citers_invalid",
                "{response}"
            );
        } else {
            assert_eq!(
                response["output"]["task_citers_available"], available,
                "{response}"
            );
        }
    }
}

#[test]
fn source_show_refuses_malformed_provenance_and_chapters_without_echoing_values() {
    for provenance in [
        "hosted",
        "{ extraction: { route: hosted } }",
        "{ unknown: secret-fixture-value }",
        "{ frames: { route: other, model: fixture } }",
    ] {
        let library = fixture(b"video", false, Default::default());
        let markdown = format!(
            "---\nid: {ID}\nkind: note\ntitle: Fixture\nsensitivity: normal\nderived_by: {provenance}\n---\nBody."
        );
        library
            .write_document(Path::new(SOURCE_PATH), &markdown)
            .unwrap();
        let response = call("office.source-show", json!({"id":ID}), &ports(library));
        assert_eq!(response["error"]["code"], "invalid_source", "{response}");
        assert!(!response.to_string().contains("secret-fixture-value"));
    }
    for chapters in [
        "## Chapter 1: Start [core]\n",
        "## Chapter 1: Start [unknown]\n\n[00:00:00](#t=00:00:00)",
        "## Chapter 1: Start [core]\n\n[00:99:00](#t=00:99:00)",
        "unsupported chapter data",
    ] {
        let library = fixture(b"video", false, Default::default());
        let markdown = library.read_text(Path::new(SOURCE_PATH)).unwrap();
        library
            .write_source_processing(
                ID,
                &office_core::domain::source::sha256_hex(markdown.as_bytes()),
                None,
                &office_core::ports::DerivedBy {
                    route: office_core::domain::endpoint::ModelRoute::Hosted,
                    model: "fixture".into(),
                },
                &[("chapters.md".into(), chapters.as_bytes().to_vec())],
            )
            .unwrap();
        let response = call("office.source-show", json!({"id":ID}), &ports(library));
        assert_eq!(response["error"]["code"], "invalid_chapters", "{response}");
    }
}

#[test]
fn source_show_checks_private_frame_image_audio_and_text_capabilities() {
    use super::video_plan::private_settings;
    use office_core::domain::endpoint::Capability;
    for (caps, mime, code) in [
        (
            vec![Capability::Text],
            "image/png",
            "private_media_unsupported",
        ),
        (vec![Capability::Text, Capability::Image], "image/png", ""),
        (
            vec![Capability::Text, Capability::Image],
            "audio/wav",
            "private_media_unsupported",
        ),
        (vec![Capability::Text, Capability::Audio], "audio/wav", ""),
        (vec![Capability::Text], "text/plain", ""),
        (
            vec![Capability::Text, Capability::Image, Capability::Audio],
            "application/octet-stream",
            "private_media_unsupported",
        ),
    ] {
        let library = fixture(b"video", true, private_settings(&caps));
        let markdown = library
            .read_text(Path::new(SOURCE_PATH))
            .unwrap()
            .replace("video/mp4", mime);
        library
            .write_document(Path::new(SOURCE_PATH), &markdown)
            .unwrap();
        let ports = ports(library);
        let response = call(
            "office.source-show",
            json!({"id":ID,"media":"original"}),
            &ports,
        );
        if code.is_empty() {
            assert_eq!(response["ok"], true, "{response}");
        } else {
            assert_eq!(response["error"]["code"], code, "{response}");
        }
        let frame = call(
            "office.source-show",
            json!({"id":ID,"media":"frame","frame":"0000.png"}),
            &ports,
        );
        if caps.contains(&Capability::Image) {
            assert_eq!(frame["ok"], true, "{frame}");
        } else {
            assert_eq!(
                frame["error"]["code"], "private_media_unsupported",
                "{frame}"
            );
        }
    }
}
