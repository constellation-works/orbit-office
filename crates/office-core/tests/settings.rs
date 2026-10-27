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
    let settings = Settings::from_yaml("retention:\n  meeting: null\nsensitivity:\n  note: private\npacks:\n  - meetings\n  - receipts\nthresholds:\n  repetition_count: 4\nopen_weights_endpoint: 'https://inference.baseten.co/v1#model'\n").expect("settings");
    assert_eq!(settings.retention["meeting"], None);
    assert_eq!(settings.retention["note"], None);
    assert_eq!(settings.sensitivity["note"].as_str(), "private");
    assert_eq!(settings.sensitivity["meeting"].as_str(), "normal");
    assert_eq!(settings.thresholds.repetition_count, 4);
    assert_eq!(settings.thresholds.window_days, 30);
    assert_eq!(settings.packs, ["meetings", "receipts"]);
    assert_eq!(
        settings.open_weights_endpoint.as_deref(),
        Some("https://inference.baseten.co/v1#model")
    );
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

#[test]
fn accepts_baseten_and_loopback_open_weights_endpoints() {
    for endpoint in [
        "https://inference.baseten.co/v1",
        "https://model-abc.api.baseten.co/v1",
        "http://127.0.0.1:9/v1",
        "http://localhost:9/v1",
        "http://[::1]:9/v1",
        "https://[::1]/v1",
    ] {
        let settings = Settings::from_yaml(&format!("open_weights_endpoint: '{endpoint}'"))
            .unwrap_or_else(|error| panic!("{endpoint}: {error}"));
        assert_eq!(settings.open_weights_endpoint.as_deref(), Some(endpoint));
    }
}

/// Open-weights hosts are the Baseten-or-loopback subset of the model
/// transport allowlist. A LAN host is not on that list, so load refuses
/// it instead of accepting a URL the HTTP guard will later reject.
#[test]
fn refuses_a_non_allowlisted_open_weights_host_at_load() {
    let endpoint = "https://lan-inference.example:8080/v1";
    let error = Settings::from_yaml(&format!("open_weights_endpoint: '{endpoint}'"))
        .expect_err("a non-allowlisted open-weights host is refused at load");
    assert!(
        !error.to_string().contains(endpoint),
        "refused endpoint was echoed: {error}"
    );
    let Error::InvalidSetting { key, reason } = error else {
        panic!("wrong error: {error:?}")
    };
    assert_eq!(key, "open_weights_endpoint");
    assert_eq!(
        reason, "expected a Baseten HTTPS endpoint or a loopback HTTP(S) endpoint",
        "a non-allowlisted open-weights host must be refused at load"
    );

    // The same boundary refuses the hosted Meta API and plain HTTP on
    // Baseten: both are outside the open-weights subset of the allowlist.
    for endpoint in ["https://api.meta.ai/v1", "http://inference.baseten.co/v1"] {
        let error = Settings::from_yaml(&format!("open_weights_endpoint: '{endpoint}'"))
            .expect_err("outside the open-weights subset");
        assert!(!error.to_string().contains(endpoint));
        let Error::InvalidSetting { key, reason } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(key, "open_weights_endpoint");
        assert_eq!(
            reason,
            "expected a Baseten HTTPS endpoint or a loopback HTTP(S) endpoint"
        );
    }
}
