use super::{block_value, refused_key};
use office_core::{Error, domain::settings::Settings};

#[test]
fn single_setting_updates_roundtrip_and_preserve_other_settings() {
    let mut settings = Settings::default();
    for (key, value) in [
        ("retention.meeting", serde_json::json!(36500)),
        ("retention.decisions", serde_json::json!(null)),
        ("sensitivity.email", serde_json::json!("private")),
        ("packs", serde_json::json!(["receipts", "meetings"])),
        ("thresholds.repetition_count", serde_json::json!(10000)),
        ("thresholds.window_days", serde_json::json!(36500)),
        ("thresholds.stale_days", serde_json::json!(1)),
        ("thresholds.friction_count", serde_json::json!(10)),
        ("thresholds.new_subject_count", serde_json::json!(5)),
        ("thresholds.unused_days", serde_json::json!(20)),
        ("open_weights", block_value()),
        (
            "open_weights.endpoint",
            serde_json::json!("http://localhost:9/v1"),
        ),
        ("open_weights.kind", serde_json::json!("own_hardware")),
        ("open_weights.label", serde_json::json!("Your hardware")),
        ("open_weights", serde_json::json!(null)),
        ("packs", serde_json::json!([])),
    ] {
        let before = serde_json::to_value(&settings).unwrap();
        let updated = settings.with_setting(key, value.clone()).unwrap();
        let mut expected = before;
        if let Some((section, name)) = key.split_once('.')
            && section != "open_weights"
        {
            expected[section][name] = value;
        } else if let Some(("open_weights", name)) = key.split_once('.') {
            expected["open_weights"][name] = value;
        } else {
            expected[key] = value;
        }
        assert_eq!(serde_json::to_value(&updated).unwrap(), expected);
        assert_eq!(
            Settings::from_yaml(&updated.to_yaml().unwrap()).unwrap(),
            updated
        );
        settings = updated;
    }
}

#[test]
fn single_setting_refuses_unknown_leaves_and_bad_values_without_mutation() {
    let settings = Settings::default();
    for key in [
        "retention",
        "sensitivity",
        "thresholds",
        "retention.unknown",
        "thresholds.unknown",
        "retention.meeting.extra",
        "meta_api_key",
        "baseten_token",
        "open_weights_token",
        "open_weights_endpoint",
        "open_weights.token",
        "secrets.meta_api_key",
        "",
    ] {
        assert!(
            matches!(
                settings.with_setting(key, serde_json::json!(10)),
                Err(Error::UnknownSetting { .. })
            ),
            "{key}"
        );
    }
    for (key, value) in [
        ("retention.meeting", serde_json::json!(0)),
        ("retention.meeting", serde_json::json!(36501)),
        ("retention.meeting", serde_json::json!("30")),
        ("retention.meeting", serde_json::json!(1.5)),
        ("sensitivity.decisions", serde_json::json!(null)),
        ("sensitivity.decisions", serde_json::json!("secret-value")),
        ("packs", serde_json::json!(["meetings", "meetings"])),
        ("packs", serde_json::json!(["unknown"])),
        ("thresholds.friction_count", serde_json::json!(10001)),
        ("thresholds.unused_days", serde_json::json!(36501)),
        ("thresholds.window_days", serde_json::json!(false)),
        (
            "open_weights",
            serde_json::json!({"endpoint": "https://secret-value@example.test", "model": "m", "kind": "company", "label": "x"}),
        ),
        ("open_weights", serde_json::json!("secret-value")),
    ] {
        let error = settings.with_setting(key, value).unwrap_err();
        assert_eq!(error.code(), "invalid_library_setting");
        assert!(!error.to_string().contains("secret-value"));
        assert_eq!(settings, Settings::default());
    }
}

#[test]
fn demo_simulated_until_takes_a_date_or_null_and_names_the_field_when_refused() {
    let settings = Settings::default();
    assert_eq!(settings.demo.simulated_until, None);
    let dated = settings
        .with_setting("demo.simulated_until", serde_json::json!("2026-10-12"))
        .unwrap();
    assert_eq!(dated.demo.simulated_until.as_deref(), Some("2026-10-12"));
    assert_eq!(
        Settings::from_yaml(&dated.to_yaml().unwrap()).unwrap(),
        dated
    );
    assert_eq!(
        dated
            .with_setting("demo.simulated_until", serde_json::json!(null))
            .unwrap(),
        settings
    );
    for value in [
        serde_json::json!("2026-02-30"),
        serde_json::json!("2026-10-12T00:00:00Z"),
        serde_json::json!("10/12/2026"),
        serde_json::json!(20261012),
        serde_json::json!(true),
    ] {
        assert_eq!(
            refused_key(settings.with_setting("demo.simulated_until", value)),
            "demo.simulated_until"
        );
    }
    assert_eq!(
        refused_key(Settings::from_yaml(
            "demo:\n  simulated_until: 2026-02-30\n"
        )),
        "demo.simulated_until"
    );
    assert_eq!(
        refused_key(Settings::from_yaml("demo:\n  unknown: 2026-10-12\n")),
        "demo.unknown"
    );
}
