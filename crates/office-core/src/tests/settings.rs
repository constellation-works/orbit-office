//! Library settings configuration through the public parser.

use office_common::Sensitivity;

use crate::domain::settings::Settings;

const LIBRARY_EXAMPLE: &str = include_str!("../../../../examples/library/.office/config.yaml");

#[test]
fn library_example_config_parses_with_its_documented_settings() {
    let settings = Settings::from_yaml(LIBRARY_EXAMPLE).expect("example Library config parses");

    assert_eq!(settings.retention["meeting"], Some(90));
    assert_eq!(settings.sensitivity["email"], Sensitivity::Private);
    assert_eq!(settings.sensitivity["receipt"], Sensitivity::Private);
    assert_eq!(settings.packs, ["meetings"]);
    assert_eq!(settings.thresholds.repetition_count, 3);
    assert_eq!(settings.thresholds.window_days, 30);
    assert_eq!(settings.open_weights, None);
}

#[test]
fn brief_task_workspaces_parse_update_and_refuse_bad_selectors() {
    use serde_json::json;
    let settings =
        Settings::from_yaml("brief:\n  task_workspaces: [ws_orbit, hm_example/ws_constellation]\n")
            .unwrap();
    assert_eq!(
        settings.brief.task_workspaces,
        ["ws_orbit", "hm_example/ws_constellation"]
    );
    let cleared = settings
        .with_setting("brief.task_workspaces", json!([]))
        .unwrap();
    assert!(cleared.brief.task_workspaces.is_empty());
    assert_eq!(
        Settings::from_yaml(&cleared.to_yaml().unwrap()).unwrap(),
        cleared
    );
    for yaml in [
        "brief:\n  task_workspaces: [ws_orbit, ws_orbit]\n",
        "brief:\n  task_workspaces: ['has space']\n",
        "brief:\n  task_workspaces: ws_orbit\n",
        "brief:\n  unknown: []\n",
    ] {
        assert!(Settings::from_yaml(yaml).is_err(), "{yaml}");
    }
    assert_eq!(
        Settings::default()
            .with_setting("brief.task_workspaces", json!(["ws_a", "ws_a"]))
            .unwrap_err()
            .code(),
        "invalid_library_setting"
    );
}

#[test]
fn invalid_open_weights_values_point_to_their_documented_limits() {
    let invalid_model = Settings::from_yaml(
        "open_weights:\n  endpoint: https://example.com/v1\n  model: 'my model'\n  kind: provider\n  label: Local\n",
    )
    .expect_err("model names must not contain whitespace");
    let invalid_label = Settings::from_yaml(&format!(
        "open_weights:\n  endpoint: https://example.com/v1\n  model: model\n  kind: provider\n  label: {}\n",
        "x".repeat(81)
    ))
    .expect_err("labels must not exceed their documented length");

    for error in [invalid_model, invalid_label] {
        assert!(
            error
                .to_string()
                .contains("docs/design/office/specs/plugin-surface.md#private-route-field-limits")
        );
    }
}

#[test]
fn script_bounds_default_roundtrip_update_and_refuse_unknown_or_excessive_values() {
    use crate::domain::settings::Settings;
    let settings = Settings::from_yaml("scripts:\n  timeout_seconds: 30\n").unwrap();
    assert_eq!(settings.scripts.timeout_seconds, 30);
    assert_eq!(settings.scripts.log_bytes, 1_048_576);
    assert_eq!(
        settings
            .with_setting("scripts.timeout_seconds", serde_json::json!(1800))
            .unwrap()
            .scripts
            .timeout_seconds,
        1800
    );
    for (key, value) in [
        ("timeout_seconds", serde_json::json!(1801)),
        ("log_bytes", serde_json::json!(1048577)),
        ("output_files", serde_json::json!(9)),
        ("output_file_bytes", serde_json::json!(1048577)),
        ("output_files", serde_json::json!(0)),
        ("unknown", serde_json::json!(1)),
    ] {
        assert!(Settings::from_yaml(&format!("scripts:\n  {key}: {value}\n")).is_err());
    }
    assert_eq!(
        Settings::from_yaml(&Settings::default_yaml()).unwrap(),
        Settings::default()
    );
}

#[test]
fn careful_model_settings_default_roundtrip_update_and_closed_validation() {
    use serde_json::json;
    let defaults = Settings::from_yaml("").unwrap();
    assert_eq!(defaults.models.careful, "muse-wonder");
    assert_eq!(
        Settings::from_yaml(&Settings::default_yaml()).unwrap(),
        defaults
    );
    let changed = defaults
        .with_setting("models.careful", json!("custom/careful-v2"))
        .unwrap();
    assert_eq!(changed.models.careful, "custom/careful-v2");
    assert_eq!(
        serde_json::from_value::<Settings>(serde_json::to_value(&changed).unwrap()).unwrap(),
        changed
    );
    for value in [
        json!(""),
        json!("invalid slug"),
        json!(42),
        json!(null),
        json!("x".repeat(129)),
    ] {
        assert_eq!(
            defaults
                .with_setting("models.careful", value)
                .unwrap_err()
                .code(),
            "invalid_library_setting"
        );
    }
    assert!(Settings::from_yaml("models:\n  unknown: something\n").is_err());
    assert_eq!(
        defaults
            .with_setting("models.unknown", json!("model"))
            .unwrap_err()
            .code(),
        "unknown_library_setting"
    );
    // Changing this tier invalidates both confirmed bootstrap approval and the
    // per-call model settings snapshot. Defaults preserve existing video plans.
    assert_ne!(
        crate::domain::source::video::settings_digest(&defaults).unwrap(),
        crate::domain::source::video::settings_digest(&changed).unwrap()
    );
}
