use super::block_value;
use office_core::domain::endpoint::Capability;
use office_core::domain::settings::Settings;
use serde_json::json;

#[test]
fn the_check_records_capabilities_and_a_new_endpoint_or_model_clears_them() {
    let configured = Settings::default()
        .with_setting("open_weights", block_value())
        .expect("block");
    let checked = configured
        .with_route_check(
            "https://muse.internal.acme.example/v1/",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Image, Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect("record");
    let route = checked.open_weights.clone().expect("block");
    assert_eq!(
        route.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert!(route.accepts(Capability::Text) && !route.accepts(Capability::Audio));
    assert_eq!(
        Settings::from_yaml(&checked.to_yaml().unwrap()).unwrap(),
        checked
    );

    let relabelled = checked
        .with_setting("open_weights.label", json!("Acme GPU cluster"))
        .expect("label");
    assert!(
        relabelled
            .open_weights
            .expect("block")
            .accepts(Capability::Text)
    );
    for (key, value) in [
        (
            "open_weights.endpoint",
            json!("https://muse2.internal.acme.example/v1"),
        ),
        ("open_weights.model", json!("another-model")),
    ] {
        let moved = checked
            .with_setting(key, value)
            .expect(key)
            .open_weights
            .expect("block");
        assert_eq!(moved.capabilities, None, "{key}");
        assert_eq!(moved.verified_at, None, "{key}");
    }

    let error = configured
        .with_route_check(
            "https://other.acme.example/v1",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect_err("checked another endpoint");
    assert_eq!(error.code(), "private_route_changed");
    assert_eq!(
        Settings::default()
            .with_route_check(
                "https://x.example/v1",
                "m",
                &[],
                "2026-10-27T14:02:00+00:00"
            )
            .expect_err("no block")
            .code(),
        "private_route_changed"
    );
}

#[test]
fn whole_block_updates_preserve_checks_only_when_endpoint_and_model_are_unchanged() {
    let configured = Settings::default()
        .with_setting("open_weights", block_value())
        .expect("block");
    let checked = configured
        .with_route_check(
            "https://muse.internal.acme.example/v1",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Image, Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect("record");

    let mut relabelled_block = block_value();
    relabelled_block["label"] = json!("Acme GPU cluster");
    let relabelled = checked
        .with_setting("open_weights", relabelled_block)
        .expect("whole-block relabel");
    let route = relabelled.open_weights.expect("block");
    assert_eq!(
        route.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert_eq!(
        route.verified_at.as_deref(),
        Some("2026-10-27T14:02:00+00:00")
    );

    for (field, value) in [
        ("endpoint", json!("https://muse2.internal.acme.example/v1")),
        ("model", json!("another-model")),
    ] {
        let mut changed_block = block_value();
        changed_block[field] = value;
        let changed = checked
            .with_setting("open_weights", changed_block)
            .expect("whole-block route change")
            .open_weights
            .expect("block");
        assert_eq!(changed.capabilities, None, "{field}");
        assert_eq!(changed.verified_at, None, "{field}");
    }

    let unchecked = configured
        .with_setting("open_weights", block_value())
        .expect("unchanged unchecked block")
        .open_weights
        .expect("block");
    assert_eq!(unchecked.capabilities, None);
    assert_eq!(unchecked.verified_at, None);
}

#[test]
fn video_defaults_and_cost_bounds() {
    let defaults = Settings::default();
    assert_eq!(defaults.packs, ["kt", "meetings"]);
    let encoded = serde_json::to_value(&defaults).unwrap();
    assert_eq!(encoded["video"]["max_frames_per_hour"], 120);
    assert_eq!(encoded["video"]["scene_threshold"], 0.3);
    for (key, value) in [
        ("video.max_frames_per_hour", json!(0)),
        ("video.max_frames_per_hour", json!(601)),
        ("video.scene_threshold", json!(0.049)),
        ("video.scene_threshold", json!(0.951)),
        ("costs.image", json!(-0.1)),
        ("costs.currency", json!("dollars")),
    ] {
        let error = defaults.with_setting(key, value).unwrap_err();
        assert_eq!(error.code(), "invalid_library_setting");
        assert!(error.to_string().contains(key));
    }
    let configured = defaults
        .with_setting("costs.image", json!(0.01))
        .unwrap()
        .with_setting("costs.currency", json!("USD"))
        .unwrap()
        .with_setting("packs", json!(["kt", "meetings"]))
        .unwrap();
    assert_eq!(
        Settings::from_yaml(&configured.to_yaml().unwrap()).unwrap(),
        configured
    );
}
