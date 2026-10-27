//! Settings through the public, pure parsing/deserialization boundary.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::{Error, domain::settings::Settings};

#[test]
fn partial_yaml_merges_policy_defaults_without_creating_observations() {
    assert_eq!(Settings::from_yaml("").expect("empty"), Settings::default());
    assert_eq!(
        Settings::from_yaml("{}").expect("empty mapping"),
        Settings::default()
    );
    let settings = Settings::from_yaml("retention:\n  meeting: null\nsensitivity:\n  note: private\npacks:\n  - meetings\n  - receipts\nthresholds:\n  repetition_count: 4\nopen_weights_endpoint: 'https://model.example.test/v1#model'\n").expect("settings");
    assert_eq!(settings.retention["meeting"], None);
    assert_eq!(settings.retention["note"], None);
    assert_eq!(settings.sensitivity["note"].as_str(), "private");
    assert_eq!(settings.sensitivity["meeting"].as_str(), "normal");
    assert_eq!(settings.thresholds.repetition_count, 4);
    assert_eq!(settings.thresholds.window_days, 30);
    assert_eq!(settings.packs, ["meetings", "receipts"]);
    let encoded = serde_json::to_value(&settings).expect("serialize");
    assert_eq!(
        serde_json::from_value::<Settings>(encoded).expect("roundtrip"),
        settings
    );
}

#[test]
fn invalid_values_name_the_exact_key_and_never_echo_the_value() {
    for (yaml, key) in [
        ("retention:\n  meeting: -1", "retention.meeting"),
        ("retention:\n  meeting: 4294967296", "retention.meeting"),
        (
            "sensitivity:\n  meeting: secret-token",
            "sensitivity.meeting",
        ),
        (
            "thresholds:\n  repetition_count: 0",
            "thresholds.repetition_count",
        ),
        ("thresholds:\n  window_days: 1.5", "thresholds.window_days"),
        ("thresholds:\n  typo: 5", "thresholds.typo"),
        ("packs: [unrecognized]", "packs"),
        ("retention:\n  typo: 2", "retention.typo"),
        (
            "open_weights_endpoint: https://secret-token@example.test",
            "open_weights_endpoint",
        ),
        ("unknown: secret-token", "unknown"),
        ("packs: [meetings, meetings]", "packs"),
        (
            "retention:\n  meeting: 1\n  meeting: 2",
            "retention.meeting",
        ),
    ] {
        let error = Settings::from_yaml(yaml).unwrap_err();
        assert!(!error.to_string().contains("secret-token"));
        let Error::InvalidSetting { key: actual, .. } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(actual, key);
    }
    assert!(serde_json::from_str::<Settings>(r#"{"thresholds":{"window_days":0}}"#).is_err());
}
