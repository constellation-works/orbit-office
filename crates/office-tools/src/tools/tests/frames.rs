//! Confirmed-plan enforcement and exact image calls through public tool dispatch.
use super::video_plan::{ID, PATH, call, confirm, fixture, preview, private_settings};
use office_core::domain::endpoint::Capability;
use office_core::domain::settings::Settings;
use office_core::domain::source::{sha256_hex, video};
use serde_json::json;
use std::path::Path;
#[test]
fn refuses_unconfirmed_then_reads_exactly_the_plan_once_and_indexes_readings() {
    let (ports, models) = fixture(Settings::default(), true, false);
    preview(&ports);
    let refused = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(refused["error"]["code"], "plan_unconfirmed");
    assert!(models.sensitivities().is_empty());
    let plan = confirm(&ports);
    let count = plan["plan"]["counts"]["frames"].as_u64().unwrap();
    let result = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["output"]["frames_read"], count);
    assert_eq!(u64::try_from(models.sensitivities().len()).unwrap(), count);
    let path = result["output"]["path"].as_str().unwrap();
    let body = ports.library.read_text(Path::new(path)).unwrap();
    assert_eq!(
        u64::try_from(body.matches("#frame=").count()).unwrap(),
        count
    );
    assert!(body.contains("[00:00:20]"));
    assert!(
        ports
            .library
            .read_text(Path::new(PATH))
            .unwrap()
            .contains("frames:")
    );
    let search = call(&ports, "office.search", json!({"query":"fake reading"}));
    assert_eq!(search["output"]["hits"][0]["id"], ID, "{search}");
    assert_eq!(call(&ports, "office.frames", json!({"id":ID})), result);
    let again = preview(&ports);
    assert_eq!(again["plan"]["digest"], plan["plan"]["digest"]);
    assert_eq!(again["plan"]["counts"]["audio"], 0);
    assert_eq!(u64::try_from(models.sensitivities().len()).unwrap(), count);
}
#[test]
fn uncertain_call_consumes_its_slot_and_cannot_be_retried() {
    let (ports, models) = fixture(Settings::default(), true, false);
    let confirmed = confirm(&ports);
    let source = ports.library.read_video(ID).unwrap();
    ports
        .library
        .mutate_video(
            ID,
            &sha256_hex(source.markdown.as_bytes()),
            video::VideoMutation::Reserve {
                digest: confirmed["plan"]["digest"].as_str().unwrap().into(),
                index: 0,
            },
        )
        .unwrap();
    let result = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(result["error"]["code"], "plan_exceeded");
    assert!(models.sensitivities().is_empty());
}
#[test]
fn changed_route_or_source_is_refused_without_a_model_call() {
    let (ports, models) = fixture(Settings::default(), true, false);
    confirm(&ports);
    ports
        .library
        .set_setting("video.max_frames_per_hour", json!(121))
        .unwrap();
    assert_eq!(
        call(&ports, "office.frames", json!({"id":ID}))["error"]["code"],
        "plan_changed"
    );
    assert!(models.sensitivities().is_empty());
}
#[test]
fn private_preview_routes_each_frame_to_the_private_model() {
    let (ports, models) = fixture(private_settings(&Capability::ALL), false, false);
    let preview = call(
        &ports,
        "office.video-plan",
        json!({"id":ID,"sensitivity":"private"}),
    );
    let digest = &preview["output"]["plan"]["digest"];
    assert_eq!(
        call(
            &ports,
            "office.video-plan",
            json!({"id":ID,"confirm":true,"digest":digest})
        )["ok"],
        true
    );
    let response = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(models.sensitivities().len(), 4);
    assert!(
        models
            .sensitivities()
            .iter()
            .all(|s| s.as_str() == "private")
    );
    let markdown = ports.library.read_text(Path::new(PATH)).unwrap();
    assert!(markdown.contains("private:company"));
    assert!(markdown.contains("fake-private"));
}
#[test]
fn conformance_goldens_replay_success_and_refusal_with_offline_media() {
    for verb in ["video-plan", "frames"] {
        let (ports, _) = fixture(Settings::default(), true, false);
        if verb == "frames" {
            confirm(&ports);
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(&path).unwrap();
        let mut input = None;
        let mut updated = Vec::new();
        let mut successes = 0;
        let mut refusals = 0;
        for line in golden.lines() {
            if let Some(raw) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str::<serde_json::Value>(raw).unwrap());
            }
            let mut replacement = None;
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(raw) = line.strip_prefix(prefix) {
                    let expected: serde_json::Value = serde_json::from_str(raw).unwrap();
                    let response = call(&ports, &format!("office.{verb}"), input.take().unwrap());
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        successes += 1;
                        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
                            replacement = Some(format!("{prefix}{}", response["output"]));
                        } else {
                            assert_eq!(response["output"], expected);
                        }
                    } else {
                        refusals += 1;
                        assert_eq!(response["error"]["code"], expected["code"]);
                    }
                }
            }
            updated.push(replacement.unwrap_or_else(|| line.into()));
        }
        assert!(successes > 0 && refusals > 0);
        if std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1") {
            std::fs::write(path, format!("{}\n", updated.join("\n"))).unwrap();
        }
    }
}

#[test]
fn failed_model_request_preserves_prior_readings_and_consumes_only_its_reserved_slot() {
    let (ports, models) = fixture(Settings::default(), true, false);
    confirm(&ports);
    models.fail_image_call_at(1);
    let response = call(&ports, "office.frames", json!({"id":ID}));
    assert_eq!(response["error"]["code"], "model_unavailable");
    assert_eq!(models.sensitivities().len(), 2);
    let source = ports.library.read_video(ID).unwrap();
    let state = video::state(&source.markdown).unwrap().unwrap();
    assert_eq!(state.frames_read, 1);
    assert_eq!(state.calls.frames, 2);
    let parent = Path::new(PATH).parent().unwrap();
    let body = ports.library.read_text(&parent.join("frames.md")).unwrap();
    assert_eq!(body.matches("#frame=").count(), 1);
    assert_eq!(
        call(&ports, "office.frames", json!({"id":ID}))["error"]["code"],
        "plan_exceeded"
    );
    assert_eq!(models.sensitivities().len(), 2);
}
